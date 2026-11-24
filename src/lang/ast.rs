// Untyped AST produced by the parser; the checker validates it in place.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::diag::Span;

/// A compound type registered in the process-wide interner: an array type (keyed by its
/// element type) or a struct or enum name. Programs in one process (LSP, parallel tests) share ids
/// safely: an array id means the same element type everywhere, and struct fields are kept per
/// program (`record::Layouts`), never here. Structs and enums share one namespace of names. Ids never appear in output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(u32);

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Array(Ty),
    Named(String),
    Param(String),
    // `List`, `Map<String, int>`: built-in types and their instances, apart from any program
    // type of the same name.
    Native(String),
}

// Append-only: (element type of an array, source name), with a hash index so N types intern in
// O(N). Bounded by the distinct array types, struct and enum names (generic instances included:
// `Pair<int, String>` is the named type of that text) and type parameters ever written.
#[derive(Default)]
struct Interner {
    entries: Vec<(Option<Ty>, &'static str)>,
    index: HashMap<Key, u32>,
    // Instance id -> (generic base type, type arguments).
    insts: HashMap<u32, (Ty, Vec<Ty>)>,
}

fn types() -> MutexGuard<'static, Interner> {
    static TYPES: OnceLock<Mutex<Interner>> = OnceLock::new();
    TYPES
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

// `name` runs before the lock is taken: naming an element type locks the interner too.
fn intern(key: Key, elem: Option<Ty>, name: String) -> TyId {
    let mut t = types();
    if let Some(&i) = t.index.get(&key) {
        return TyId(i);
    }
    let i = t.entries.len() as u32;
    t.entries.push((elem, Box::leak(name.into_boxed_str())));
    t.index.insert(key, i);
    TyId(i)
}

fn named(name: &str) -> TyId {
    intern(Key::Named(name.to_string()), None, name.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ty {
    Int,
    Double,
    Boolean,
    // Immutable text; a build-time value like `int`.
    String,
    Bit,
    BitReg,
    Qubit,
    QReg,
    Void,
    // Per-shot views of a `BitReg` (`r.asInt()`, `r.toBinary()`); printable only.
    ShotInt,
    ShotBinary,
    // Placeholder after an error, so one mistake does not cascade.
    Error,
    // `int[]`, `double[][]`: a reference to a mutable array of classical values.
    Array(TyId),
    // `Point`: a reference to a value of a `struct` declared in the program.
    Struct(TyId),
    // `Shape`: an immutable value of an `enum` declared in the program.
    Enum(TyId),
    // `T` inside a generic declaration: an opaque classical type until monomorphization.
    Param(TyId),
    // `Shape`: a reference to a value of any struct or enum that implements the `interface`.
    Iface(TyId),
    // `List<int>`, `Map<String, int>`, `Random`: built-in reference types (src/lang/native.rs).
    Native(TyId),
}

impl Ty {
    /// The array type with elements of type `elem`.
    pub fn array_of(elem: Ty) -> Ty {
        let name = format!("{}[]", elem.name());
        Ty::Array(intern(Key::Array(elem), Some(elem), name))
    }

    /// The type of the `struct` named `name`.
    pub fn struct_named(name: &str) -> Ty {
        Ty::Struct(named(name))
    }

    /// The type of the `enum` named `name`.
    pub fn enum_named(name: &str) -> Ty {
        Ty::Enum(named(name))
    }

    /// The type of the `interface` named `name`.
    pub fn iface_named(name: &str) -> Ty {
        Ty::Iface(named(name))
    }

    /// The built-in type named `name` (`List`, `Map`, `Random`), without type arguments.
    pub fn native(name: &str) -> Ty {
        Ty::Native(intern(
            Key::Native(name.to_string()),
            None,
            name.to_string(),
        ))
    }

    /// The type parameter named `name`.
    pub fn param_named(name: &str) -> Ty {
        Ty::Param(intern(Key::Param(name.to_string()), None, name.to_string()))
    }

    /// `Base<args>`: an instance of the generic struct or enum `base`. Its name is the source
    /// text, so `struct_named("Pair<int, String>")` is the same type.
    pub fn instance(base: Ty, args: &[Ty]) -> Ty {
        let names: Vec<&str> = args.iter().map(|a| a.name()).collect();
        let name = format!("{}<{}>", base.name(), names.join(", "));
        let id = match base {
            Ty::Native(_) => intern(Key::Native(name.clone()), None, name),
            _ => named(&name),
        };
        types()
            .insts
            .entry(id.0)
            .or_insert_with(|| (base, args.to_vec()));
        match base {
            Ty::Enum(_) => Ty::Enum(id),
            Ty::Iface(_) => Ty::Iface(id),
            Ty::Native(_) => Ty::Native(id),
            _ => Ty::Struct(id),
        }
    }

    /// The generic base and type arguments of an instance type.
    pub fn generic_args(self) -> Option<(Ty, Vec<Ty>)> {
        match self {
            Ty::Struct(TyId(i)) | Ty::Enum(TyId(i)) | Ty::Iface(TyId(i)) | Ty::Native(TyId(i)) => {
                types().insts.get(&i).cloned()
            }
            _ => None,
        }
    }

    /// The type with each type parameter replaced by `f` (unchanged where `f` gives `None`).
    pub fn subst(self, f: &dyn Fn(Ty) -> Option<Ty>) -> Ty {
        if !self.has_param() {
            return self;
        }
        match self {
            Ty::Param(_) => f(self).unwrap_or(self),
            Ty::Array(_) => match self.elem() {
                Some(e) => Ty::array_of(e.subst(f)),
                None => self,
            },
            _ => match self.generic_args() {
                Some((base, args)) => {
                    let args: Vec<Ty> = args.iter().map(|a| a.subst(f)).collect();
                    Ty::instance(base, &args)
                }
                None => self,
            },
        }
    }

    /// Whether a type parameter occurs in the type.
    pub fn has_param(self) -> bool {
        match self {
            Ty::Param(_) => true,
            Ty::Array(_) => self.elem().is_some_and(Ty::has_param),
            Ty::Struct(_) | Ty::Enum(_) | Ty::Iface(_) | Ty::Native(_) => self
                .generic_args()
                .is_some_and(|(_, a)| a.iter().any(|t| t.has_param())),
            _ => false,
        }
    }

    /// The number of type constructors in the type (`Pair<int, int[]>` is 4).
    pub fn size(self) -> usize {
        match self {
            Ty::Array(_) => 1 + self.elem().map_or(0, Ty::size),
            Ty::Struct(_) | Ty::Enum(_) | Ty::Iface(_) | Ty::Native(_) => {
                1 + self
                    .generic_args()
                    .map_or(0, |(_, a)| a.iter().map(|t| t.size()).sum())
            }
            _ => 1,
        }
    }

    /// The element type of an array type.
    pub fn elem(self) -> Option<Ty> {
        match self {
            Ty::Array(TyId(i)) => types().entries.get(i as usize).and_then(|(e, _)| *e),
            _ => None,
        }
    }

    /// Types arrays, struct fields and enum payloads may hold: build-time classical values,
    /// arrays, structs and enums.
    pub fn is_element(self) -> bool {
        matches!(
            self,
            Ty::Int
                | Ty::Double
                | Ty::Boolean
                | Ty::String
                | Ty::Array(_)
                | Ty::Struct(_)
                | Ty::Enum(_)
                | Ty::Param(_)
                | Ty::Iface(_)
                | Ty::Native(_)
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Ty::Array(TyId(i))
            | Ty::Struct(TyId(i))
            | Ty::Enum(TyId(i))
            | Ty::Param(TyId(i))
            | Ty::Iface(TyId(i))
            | Ty::Native(TyId(i)) => types()
                .entries
                .get(i as usize)
                .map_or("{error}", |(_, n)| *n),
            Ty::Int => "int",
            Ty::Double => "double",
            Ty::Boolean => "boolean",
            Ty::String => "String",
            Ty::Bit => "Bit",
            Ty::BitReg => "BitReg",
            Ty::Qubit => "Qubit",
            Ty::QReg => "QReg",
            Ty::Void => "void",
            Ty::ShotInt => "BitReg.asInt()",
            Ty::ShotBinary => "BitReg.toBinary()",
            Ty::Error => "{error}",
        }
    }

    /// Type names as written in source (`void` is only valid as a return type).
    pub fn from_name(s: &str) -> Option<Ty> {
        Some(match s {
            "int" => Ty::Int,
            "double" => Ty::Double,
            "boolean" => Ty::Boolean,
            "String" => Ty::String,
            "Bit" => Ty::Bit,
            "BitReg" => Ty::BitReg,
            "Qubit" => Ty::Qubit,
            "QReg" => Ty::QReg,
            "void" => Ty::Void,
            _ => return None,
        })
    }

    pub fn is_quantum(self) -> bool {
        matches!(self, Ty::Qubit | Ty::QReg)
    }

    pub fn is_shot_time(self) -> bool {
        matches!(self, Ty::Bit | Ty::BitReg | Ty::ShotInt | Ty::ShotBinary)
    }

    pub fn is_numeric(self) -> bool {
        matches!(self, Ty::Int | Ty::Double)
    }
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct Program {
    pub imports: Vec<Import>,
    pub structs: Vec<StructDecl>,
    pub enums: Vec<EnumDecl>,
    pub ifaces: Vec<IfaceDecl>,
    pub fns: Vec<FnDecl>,
}

impl Program {
    /// Moves the methods of every struct and enum into `fns` as ordinary functions: `len` of
    /// `Vec2` becomes `Vec2.len` with a first parameter `this` of the type (`Box<T>` for a generic
    /// type, whose parameters the method shares). Linking and the IDE call this once the bodies
    /// are final; later passes see methods only through `FnDecl::method`.
    pub fn lift_methods(&mut self) {
        let mut lifted = Vec::new();
        let owners = self
            .structs
            .iter_mut()
            .map(|s| {
                (
                    Ty::struct_named(&s.name.name),
                    &s.name,
                    &s.params,
                    &mut s.methods,
                )
            })
            .chain(self.enums.iter_mut().map(|e| {
                (
                    Ty::enum_named(&e.name.name),
                    &e.name,
                    &e.params,
                    &mut e.methods,
                )
            }));
        for (base, name, params, methods) in owners {
            let this = if params.is_empty() {
                base
            } else {
                let args: Vec<Ty> = params.iter().map(|p| Ty::param_named(&p.name)).collect();
                Ty::instance(base, &args)
            };
            for mut f in std::mem::take(methods) {
                let method = std::mem::take(&mut f.name.name);
                f.name.name = format!("{}.{method}", name.name);
                f.tparams = params.clone();
                f.bounds = vec![None; params.len()];
                f.params.insert(
                    0,
                    Param {
                        consume: None,
                        inout: f.mutating,
                        name: Ident {
                            name: "this".to_string(),
                            span: f.name.span,
                        },
                        ty: this,
                        ty_span: name.span,
                    },
                );
                f.method = Some(method);
                lifted.push(f);
            }
        }
        self.fns.extend(lifted);
    }
}

/// `interface Shape { double area(); }` or `interface Ordered<T> { int compare(T other); }`
#[derive(Debug, Clone)]
pub struct IfaceDecl {
    pub name: Ident,
    // Type parameters (`<T>`), empty for an ordinary interface.
    pub params: Vec<Ident>,
    pub methods: Vec<MethodSig>,
}

/// A method an interface requires: `double area();`
#[derive(Debug, Clone)]
pub struct MethodSig {
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub ret_span: Span,
}

/// `enum Shape { Circle(double r), Empty }` or `enum Option<T> { None, Some(T value) }`
#[derive(Debug, Clone)]
pub struct EnumDecl {
    pub name: Ident,
    // Type parameters (`<T>`), empty for an ordinary enum.
    pub params: Vec<Ident>,
    // `implements Shape, Ordered<Money>`: the interfaces and where each is written.
    pub implements: Vec<(Ty, Span)>,
    pub variants: Vec<Variant>,
    // Methods declared after the variants (`Program::lift_methods` moves them to `fns`).
    pub methods: Vec<FnDecl>,
    // `Result` or `Option` of the prelude (`link::add_prelude`), not a program's own type.
    pub prelude: bool,
}

/// One variant of an `enum`: its payload fields are named like parameters.
#[derive(Debug, Clone)]
pub struct Variant {
    pub name: Ident,
    pub fields: Vec<Field>,
}

/// `struct Point { double x; double y; }` or `struct Pair<A, B> { A first; B second; }`
#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: Ident,
    // Type parameters (`<A, B>`), empty for an ordinary struct.
    pub params: Vec<Ident>,
    // `implements Shape, Ordered<Money>`: the interfaces and where each is written.
    pub implements: Vec<(Ty, Span)>,
    pub fields: Vec<Field>,
    // Methods declared after the fields (`Program::lift_methods` moves them to `fns`).
    pub methods: Vec<FnDecl>,
    // The prelude's `Shared<T>` (`--experimental value-semantics`), not a program's own struct.
    pub prelude: bool,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub ty: Ty,
    pub ty_span: Span,
    pub name: Ident,
}

/// `import pkg.module.function;` or `import pkg.module.*;`
#[derive(Debug, Clone)]
pub struct Import {
    // Every segment before the function name (or `*`): the package, then nested modules.
    pub module: Vec<Ident>,
    // `None` for a wildcard import.
    pub item: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
    // Span of the `public` keyword: other modules may import the function.
    pub public: Option<Span>,
    // Type parameters of a generic function (`<T> T first(T[] a)`), empty otherwise.
    pub tparams: Vec<Ident>,
    // The bound of each type parameter (`<T extends Shape>`), `None` where there is none.
    pub bounds: Vec<Option<(Ty, Span)>>,
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub ret_span: Span,
    pub body: Block,
    // For a method moved out of its type (`Vec2.len`): the method's own name (`len`). Its first
    // parameter is `this`, of the type that declares it.
    pub method: Option<String>,
    // Span of `inout` before a method's return type (`--experimental value-semantics` only): the
    // method may change `this`, whose parameter then has `inout`.
    pub mutating: Option<Span>,
}

#[derive(Debug, Clone)]
pub struct Param {
    // Span of the `consume` keyword: the function takes ownership of the qubit(s) (with
    // `--experimental value-semantics`, of any value).
    pub consume: Option<Span>,
    // Span of the `inout` word (`--experimental value-semantics` only): the function may change
    // the caller's value.
    pub inout: Option<Span>,
    pub name: Ident,
    pub ty: Ty,
    pub ty_span: Span,
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    // Span of the closing brace: where block-local qubits go out of scope.
    pub close: Span,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Decl {
        is_final: bool,
        // `None` for `var`: inferred from the initializer (classical types only).
        ty: Option<Ty>,
        ty_span: Span,
        name: Ident,
        init: Expr,
    },
    Assign {
        name: Ident,
        value: Expr,
    },
    If(IfStmt),
    For {
        var: Ident,
        start: Expr,
        end: Expr,
        body: Block,
    },
    // `while cond { ... }`; `span` is the `while` keyword.
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    Break {
        span: Span,
    },
    Continue {
        span: Span,
    },
    Return {
        value: Option<Expr>,
        span: Span,
    },
    // `p.x = v;` or `p.x op= v;` (`base` may be any expression of a struct type).
    SetField {
        base: Expr,
        name: Ident,
        op: Option<BinOp>,
        value: Expr,
        span: Span,
    },
    // `a[i] = v;` or `a[i] op= v;` (`base` may itself be an index, as in `m[i][j] = v`).
    SetIndex {
        base: Expr,
        index: Expr,
        op: Option<BinOp>,
        value: Expr,
        span: Span,
    },
    // `match e { pattern => { ... } ... }`; `span` is the `match` keyword.
    Match {
        scrutinee: Expr,
        arms: Vec<Arm>,
        span: Span,
    },
    Expr(Expr),
}

#[derive(Debug, Clone)]
pub struct Arm {
    pub pat: Pattern,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Pattern {
    pub kind: PatKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum PatKind {
    // `_`
    Wild,
    Int(i64),
    // A `String` literal (the pieces of its text; a `{}` hole is an error).
    Str(Vec<FmtPiece>),
    Bool(bool),
    // `Circle(r, _)` or `Shape.Circle(r, _)`: `_` bindings are ignored.
    Variant {
        qual: Option<Ident>,
        name: Ident,
        binds: Vec<Ident>,
    },
}

#[derive(Debug, Clone)]
pub struct IfStmt {
    pub cond: Expr,
    pub then: Block,
    pub els: Option<Else>,
}

#[derive(Debug, Clone)]
pub enum Else {
    Block(Block),
    If(Box<IfStmt>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FmtPiece {
    Lit(String),
    Hole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    // `~x` on `int`.
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    // Bitwise `&`, `|`, `^` and shifts `<<`, `>>` (arithmetic), `>>>` (logical) on `int`.
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    UShr,
}

impl BinOp {
    pub fn is_bitwise(self) -> bool {
        matches!(
            self,
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr | BinOp::UShr
        )
    }
}

#[derive(Debug, Clone)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum ExprKind {
    Int(i64),
    Double(f64),
    Bool(bool),
    Str(Vec<FmtPiece>),
    Var(String),
    Call {
        name: Ident,
        args: Vec<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    // `r.asInt()` on a value, or `fs.readString(p)` on a module alias (rewritten to a `Call`
    // by the linker).
    Method {
        recv: Box<Expr>,
        name: Ident,
        args: Vec<Expr>,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Cast {
        ty: Ty,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    // `new Point(1.0, 2.0)`: the fields in declaration order.
    NewStruct {
        ty: Ty,
        args: Vec<Expr>,
    },
    // `Shape.Circle(2.0)` or `Shape.Empty`: the payload in declaration order.
    Variant {
        ty: Ty,
        name: Ident,
        args: Vec<Expr>,
    },
    // `p.x`
    Field {
        recv: Box<Expr>,
        name: Ident,
    },
    // `e?` on a `Result` or `Option`: the `Ok`/`Some` payload, or an early return of the
    // `Err`/`None` from the enclosing function. `op` is the `?` token.
    Try {
        expr: Box<Expr>,
        op: Span,
    },
    // `new int[n]`, `new double[r][c]`, `new int[n][]`: `ty` is the resulting array type; one
    // length per given dimension, missing inner dimensions are empty arrays.
    NewArray {
        ty: Ty,
        dims: Vec<Expr>,
    },
    // `new int[]{1, 2}` (`ty` given) or the initializer `{1, 2}` of a declaration (`ty` is `None`
    // until the checker fills it in from the declared type).
    ArrayLit {
        ty: Option<Ty>,
        elems: Vec<Expr>,
    },
    // `inout x` as a call argument (`--experimental value-semantics` only): the argument of an
    // `inout` parameter. `values::lower` removes it before any engine runs.
    Inout {
        expr: Box<Expr>,
    },
}

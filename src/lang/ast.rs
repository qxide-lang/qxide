// Untyped AST produced by the parser; the checker validates it in place.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

use super::diag::Span;

/// A compound type registered in the process-wide interner: an array type (keyed by its
/// element type) or a struct name. Programs in one process (LSP, parallel tests) share ids
/// safely: an array id means the same element type everywhere, and struct fields are kept per
/// program (`record::Layouts`), never here. Ids never appear in output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TyId(u32);

#[derive(Clone, PartialEq, Eq, Hash)]
enum Key {
    Array(Ty),
    Struct(String),
}

// Append-only: (element type of an array, source name), with a hash index so N types intern in
// O(N). Bounded by the distinct array types and struct names ever written.
#[derive(Default)]
struct Interner {
    entries: Vec<(Option<Ty>, &'static str)>,
    index: HashMap<Key, u32>,
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
    // Per-shot views of a `BitReg` (`r.as_int()`, `r.to_binary()`); printable only.
    ShotInt,
    ShotBinary,
    // Placeholder after an error, so one mistake does not cascade.
    Error,
    // `int[]`, `double[][]`: a reference to a mutable array of classical values.
    Array(TyId),
    // `Point`: a reference to a value of a `struct` declared in the program.
    Struct(TyId),
}

impl Ty {
    /// The array type with elements of type `elem`.
    pub fn array_of(elem: Ty) -> Ty {
        let name = format!("{}[]", elem.name());
        Ty::Array(intern(Key::Array(elem), Some(elem), name))
    }

    /// The type of the `struct` named `name`.
    pub fn struct_named(name: &str) -> Ty {
        Ty::Struct(intern(
            Key::Struct(name.to_string()),
            None,
            name.to_string(),
        ))
    }

    /// The element type of an array type.
    pub fn elem(self) -> Option<Ty> {
        match self {
            Ty::Array(TyId(i)) => types().entries.get(i as usize).and_then(|(e, _)| *e),
            _ => None,
        }
    }

    /// Types arrays and struct fields may hold: build-time classical values, arrays and structs.
    pub fn is_element(self) -> bool {
        matches!(
            self,
            Ty::Int | Ty::Double | Ty::Boolean | Ty::String | Ty::Array(_) | Ty::Struct(_)
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            Ty::Array(TyId(i)) | Ty::Struct(TyId(i)) => types()
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
            Ty::ShotInt => "BitReg.as_int()",
            Ty::ShotBinary => "BitReg.to_binary()",
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
    pub fns: Vec<FnDecl>,
}

/// `struct Point { double x; double y; }`
#[derive(Debug, Clone)]
pub struct StructDecl {
    pub name: Ident,
    pub fields: Vec<Field>,
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
    pub name: Ident,
    pub params: Vec<Param>,
    pub ret: Ty,
    pub ret_span: Span,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct Param {
    // Span of the `consume` keyword: the function takes ownership of the qubit(s).
    pub consume: Option<Span>,
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
    Expr(Expr),
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
    // `r.as_int()` on a value, or `fs.read_text(p)` on a module alias (rewritten to a `Call`
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
    // `p.x`
    Field {
        recv: Box<Expr>,
        name: Ident,
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
}

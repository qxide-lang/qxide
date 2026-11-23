// Recursive-descent parser; stops at the first syntax error.

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::lexer::{Tok, Token};
use super::msg;
use super::{ffi, native};

use std::collections::HashSet;

// Bounds recursion on hostile input (deeply nested parentheses or blocks).
pub const MAX_DEPTH: usize = 128;

/// Types of the prelude (`std/prelude.qx`), usable in every file without an import.
pub const PRELUDE_TYPES: &[&str] = &["Result", "Option"];

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
    // Names of the structs and enums this file declares (found before parsing, so types may be
    // used before their declaration).
    structs: HashSet<String>,
    enums: HashSet<String>,
    ifaces: HashSet<String>,
    // The structs, enums and interfaces declared with type parameters: only after one of these names is `<`
    // a list of type arguments (`Pair<int, String>`), so `a < b` stays a comparison.
    generics: HashSet<String>,
    // Type parameters in scope (of the generic declaration being parsed).
    tparams: Vec<String>,
    // Numbers the hidden variables of `for x in xs` loops.
    fresh: usize,
}

type PResult<T> = Result<T, Diagnostic>;

// `extends I` of one type parameter, with the span of `I`.
type Bound = Option<(Ty, Span)>;

pub fn parse(toks: Vec<Token>) -> PResult<Program> {
    let mut structs = HashSet::new();
    let mut enums = HashSet::new();
    let mut ifaces = HashSet::new();
    let mut generics = HashSet::new();
    let mut depth = 0usize;
    for (i, w) in toks.windows(2).enumerate() {
        let set = match &w[0].tok {
            Tok::LBrace => {
                depth += 1;
                continue;
            }
            Tok::RBrace => {
                depth = depth.saturating_sub(1);
                continue;
            }
            Tok::Struct if depth == 0 => &mut structs,
            Tok::Enum if depth == 0 => &mut enums,
            Tok::Ident(w) if depth == 0 && w == INTERFACE => &mut ifaces,
            _ => continue,
        };
        if let Tok::Ident(n) = &w[1].tok {
            set.insert(n.clone());
            if toks.get(i + 2).is_some_and(|t| t.tok == Tok::Lt) {
                generics.insert(n.clone());
            }
        }
    }
    // The prelude's generic enums are known in every file that does not declare its own type of
    // that name (the linker adds their declarations, `link::add_prelude`).
    for name in PRELUDE_TYPES {
        if !structs.contains(*name) && !enums.contains(*name) && !ifaces.contains(*name) {
            enums.insert(name.to_string());
            generics.insert(name.to_string());
        }
    }
    // Built-in `List<T>`, `Map<K, V>` and `Random` (src/lang/native.rs), unless the file
    // declares its own type of that name.
    for name in native::GENERIC {
        if !structs.contains(*name) && !enums.contains(*name) && !ifaces.contains(*name) {
            generics.insert(name.to_string());
        }
    }
    let mut p = Parser {
        toks,
        pos: 0,
        depth: 0,
        structs,
        enums,
        ifaces,
        generics,
        tparams: Vec::new(),
        fresh: 0,
    };
    let mut imports = Vec::new();
    while p.peek() == &Tok::Import {
        imports.push(p.import()?);
    }
    let mut fns = Vec::new();
    let mut structs = Vec::new();
    let mut enums = Vec::new();
    let mut ifaces = Vec::new();
    while p.peek() != &Tok::Eof {
        if p.peek() == &Tok::Import {
            return Err(Diagnostic::new(
                Code::MisplacedImport,
                p.span(),
                msg::misplaced_import(),
            ));
        }
        if p.peek() == &Tok::Struct {
            structs.push(p.struct_decl()?);
        } else if p.peek() == &Tok::Enum {
            enums.push(p.enum_decl()?);
        } else if p.at_interface() {
            ifaces.push(p.iface_decl()?);
        } else if p.at_extern() {
            let (wrapper, stub) = p.extern_decl()?;
            fns.push(wrapper);
            fns.push(stub);
        } else {
            fns.push(p.fn_decl()?);
        }
    }
    Ok(Program {
        imports,
        structs,
        enums,
        ifaces,
        fns,
    })
}

// Contextual keywords: ordinary identifiers everywhere else, so existing names keep working.
const INTERFACE: &str = "interface";
// `extern "lib" ...` at the top level: an identifier followed by a string is no other item.
const EXTERN: &str = "extern";
const IMPLEMENTS: &str = "implements";
const EXTENDS: &str = "extends";

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn bump(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.peek() == t {
            self.bump();
            true
        } else {
            false
        }
    }

    fn unexpected(&self, message: String) -> Diagnostic {
        Diagnostic::new(Code::UnexpectedToken, self.span(), message)
    }

    fn expect(&mut self, t: Tok) -> PResult<Span> {
        if self.peek() == &t {
            Ok(self.bump().span)
        } else {
            let want = format!("`{}`", t.symbol().unwrap_or_default());
            Err(self.unexpected(msg::expected_token(&want, &self.peek().describe())))
        }
    }

    fn ident(&mut self) -> PResult<Ident> {
        match self.peek().clone() {
            Tok::Ident(name) => {
                let span = self.bump().span;
                Ok(Ident { name, span })
            }
            t => Err(self.unexpected(msg::expected_ident(&t.describe()))),
        }
    }

    // Declared names: type names are rejected so that `(int) x` is always a cast.
    fn name(&mut self) -> PResult<Ident> {
        let id = self.ident()?;
        if Ty::from_name(&id.name).is_some() {
            return Err(Diagnostic::new(
                Code::TypeNameAsIdent,
                id.span,
                msg::type_name_as_ident(&id.name),
            ));
        }
        Ok(id)
    }

    fn ty(&mut self) -> PResult<(Ty, Span)> {
        match self.peek().clone() {
            // Local declarations handle `var` before calling `ty`.
            Tok::Var => Err(Diagnostic::new(
                Code::VarNotAllowed,
                self.span(),
                msg::var_not_allowed(),
            )),
            Tok::Ident(name) => {
                let span = self.bump().span;
                let Some(mut t) = self.named_ty(&name) else {
                    return Err(Diagnostic::new(
                        Code::UnknownType,
                        span,
                        msg::unknown_type(&name),
                    ));
                };
                let mut end = span;
                // `Pair<int, String>`: the arguments of a generic type.
                if self.generics.contains(&name)
                    && !self.tparams.contains(&name)
                    && self.peek() == &Tok::Lt
                {
                    let (args, close) = self.type_args()?;
                    t = Ty::instance(t, &args);
                    end = close;
                }
                // `int[]`, `double[][]`: each `[]` wraps the type in an array.
                while self.peek() == &Tok::LBracket && self.peek_at(1) == &Tok::RBracket {
                    self.enter()?;
                    self.bump();
                    end = self.bump().span;
                    t = Ty::array_of(t);
                }
                self.depth -= array_depth(t);
                Ok((t, span.to(end)))
            }
            t => Err(self.unexpected(msg::expected_type(&t.describe()))),
        }
    }

    // `<int, String>`: type arguments, through the closing `>`.
    fn type_args(&mut self) -> PResult<(Vec<Ty>, Span)> {
        self.enter()?;
        self.expect(Tok::Lt)?;
        let mut args = vec![self.ty()?.0];
        while self.eat(&Tok::Comma) {
            args.push(self.ty()?.0);
        }
        let end = self.expect(Tok::Gt)?;
        self.depth -= 1;
        Ok((args, end))
    }

    // `<A, B>`: the type parameters of a generic declaration (none without a `<`).
    fn type_params(&mut self) -> PResult<Vec<Ident>> {
        let mut params = Vec::new();
        if self.eat(&Tok::Lt) {
            params.push(self.name()?);
            while self.eat(&Tok::Comma) {
                params.push(self.name()?);
            }
            self.expect(Tok::Gt)?;
        }
        Ok(params)
    }

    // `<T, U extends Shape>`: the type parameters of a generic function and their bounds. Each
    // parameter is in scope from its own bound on (`<T extends Ordered<T>>`).
    fn bounded_type_params(&mut self) -> PResult<(Vec<Ident>, Vec<Bound>)> {
        let (mut params, mut bounds) = (Vec::new(), Vec::new());
        self.tparams.clear();
        if self.eat(&Tok::Lt) {
            loop {
                let p = self.name()?;
                self.tparams.push(p.name.clone());
                params.push(p);
                bounds.push(if self.eat_word(EXTENDS) {
                    Some(self.ty()?)
                } else {
                    None
                });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
            self.expect(Tok::Gt)?;
        }
        Ok((params, bounds))
    }

    fn eat_word(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Tok::Ident(w) if w == word) {
            self.bump();
            true
        } else {
            false
        }
    }

    // `interface Name` at the start of a top-level item.
    fn at_interface(&self) -> bool {
        matches!(self.peek(), Tok::Ident(w) if w == INTERFACE)
            && matches!(self.peek_at(1), Tok::Ident(_))
    }

    // `implements A, B<int>` after the name of a struct or enum (none without the word).
    fn implements(&mut self) -> PResult<Vec<(Ty, Span)>> {
        let mut out = Vec::new();
        if self.eat_word(IMPLEMENTS) {
            out.push(self.ty()?);
            while self.eat(&Tok::Comma) {
                out.push(self.ty()?);
            }
        }
        Ok(out)
    }

    // `interface Name<T> { Ret method(Type a); ... }`
    fn iface_decl(&mut self) -> PResult<IfaceDecl> {
        self.bump();
        let name = self.name()?;
        let params = self.type_params()?;
        self.tparams = params.iter().map(|p| p.name.clone()).collect();
        self.expect(Tok::LBrace)?;
        let mut methods = Vec::new();
        while self.peek() != &Tok::RBrace {
            let (ret, ret_span) = self.ty()?;
            let name = self.name()?;
            let params = self.params()?;
            self.expect(Tok::Semi)?;
            methods.push(MethodSig {
                name,
                params,
                ret,
                ret_span,
            });
        }
        self.expect(Tok::RBrace)?;
        self.tparams.clear();
        Ok(IfaceDecl {
            name,
            params,
            methods,
        })
    }

    fn at_extern(&self) -> bool {
        matches!(self.peek(), Tok::Ident(n) if n == EXTERN)
            && matches!(self.peek_at(1), Tok::Str(_))
    }

    // `extern "lib" double cos(double x);` (src/lang/ffi.rs): a function `cos` whose body calls
    // the std stub `std.ffi.lib@cos(d)d`, plus that stub. The linker checks the types and the
    // permission; the host loads the library.
    fn extern_decl(&mut self) -> PResult<(FnDecl, FnDecl)> {
        let kw = self.bump().span;
        let t = self.bump();
        // `at_extern` saw the string.
        let lib = match t.tok {
            Tok::Str(s) => s,
            _ => String::new(),
        };
        let lib_span = t.span;
        let (ret, ret_span) = self.ty()?;
        let name = self.name()?;
        let params = self.params()?;
        let semi = self.expect(Tok::Semi)?;
        let span = kw.to(semi);
        let tys: Vec<Ty> = params.iter().map(|p| p.ty).collect();
        let stub_name = ffi::stub_name(&lib, &name.name, &tys, ret);
        let call = Expr {
            kind: ExprKind::Call {
                name: Ident {
                    name: stub_name.clone(),
                    span: name.span,
                },
                args: params
                    .iter()
                    .map(|p| Expr {
                        kind: ExprKind::Var(p.name.name.clone()),
                        span: p.name.span,
                    })
                    .collect(),
            },
            span,
        };
        let body = if ret == Ty::Void {
            vec![Stmt::Expr(call)]
        } else {
            vec![Stmt::Return {
                value: Some(call),
                span,
            }]
        };
        let default = match ret {
            Ty::Int => Some(ExprKind::Int(0)),
            Ty::Double => Some(ExprKind::Double(0.0)),
            _ => None,
        };
        let stub_body = default.map_or_else(Vec::new, |kind| {
            vec![Stmt::Return {
                value: Some(Expr { kind, span }),
                span,
            }]
        });
        let stub = FnDecl {
            public: None,
            tparams: Vec::new(),
            bounds: Vec::new(),
            name: Ident {
                name: stub_name,
                span: lib_span,
            },
            params: params.clone(),
            ret,
            ret_span,
            body: Block {
                stmts: stub_body,
                close: semi,
            },
            method: None,
        };
        let wrapper = FnDecl {
            public: None,
            tparams: Vec::new(),
            bounds: Vec::new(),
            name,
            params,
            ret,
            ret_span,
            body: Block {
                stmts: body,
                close: semi,
            },
            method: None,
        };
        Ok((wrapper, stub))
    }

    // `(Type a, consume Type b)`
    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect(Tok::LParen)?;
        let mut params = Vec::new();
        while self.peek() != &Tok::RParen {
            let consume = if self.peek() == &Tok::Consume {
                Some(self.bump().span)
            } else {
                None
            };
            let (ty, ty_span) = self.ty()?;
            let pname = self.name()?;
            params.push(Param {
                consume,
                name: pname,
                ty,
                ty_span,
            });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen)?;
        Ok(params)
    }

    // A method of a struct or enum, after its return type and name: `(params) { ... }`. The
    // type parameters of the declaring type stay in scope.
    fn method(&mut self, ret: Ty, ret_span: Span, name: Ident) -> PResult<FnDecl> {
        let params = self.params()?;
        let body = self.block()?;
        Ok(FnDecl {
            public: None,
            tparams: Vec::new(),
            bounds: Vec::new(),
            name,
            params,
            ret,
            ret_span,
            body,
            method: None,
        })
    }

    // `new int[n][m]`, `new int[n][]` or `new int[]{1, 2}`, after the `new`.
    fn new_array(&mut self, start: Span) -> PResult<Expr> {
        let (elem, _) = match self.peek() {
            Tok::Ident(n) if self.is_type_name(n) => self.ty()?,
            t => return Err(self.unexpected(msg::expected_type(&t.describe()))),
        };
        // `new Point(1.0, 2.0)`
        if matches!(elem, Ty::Struct(_) | Ty::Native(_)) && self.peek() == &Tok::LParen {
            self.bump();
            let (args, end) = self.call_args()?;
            return Ok(Expr {
                kind: ExprKind::NewStruct { ty: elem, args },
                span: start.to(end),
            });
        }
        let mut ty = elem;
        let mut dims = Vec::new();
        let mut end = self.prev_span();
        let mut entered = 0;
        while self.peek() == &Tok::LBracket {
            self.enter()?;
            entered += 1;
            self.bump();
            if self.peek() == &Tok::RBracket {
                end = self.bump().span;
            } else if dims.len() + 1 == array_depth(ty) + 1 {
                dims.push(self.expr()?);
                end = self.expect(Tok::RBracket)?;
            } else {
                // Lengths come first: `new int[n][]`, never `new int[][n]`.
                return Err(self.unexpected(msg::expected_token("`]`", &self.peek().describe())));
            }
            ty = Ty::array_of(ty);
        }
        self.depth -= entered;
        if array_depth(ty) == 0 {
            return Err(self.unexpected(msg::expected_token("`[`", &self.peek().describe())));
        }
        if dims.is_empty() {
            if self.peek() != &Tok::LBrace {
                return Err(self.unexpected(msg::expected_token("`{`", &self.peek().describe())));
            }
            let lit = self.array_lit(Some(ty))?;
            return Ok(Expr {
                span: start.to(lit.span),
                kind: lit.kind,
            });
        }
        Ok(Expr {
            kind: ExprKind::NewArray { ty, dims },
            span: start.to(end),
        })
    }

    // `{a, b, c}` for an array of type `ty` (`None` when the type is unknown); nested braces
    // are the inner arrays.
    fn array_lit(&mut self, ty: Option<Ty>) -> PResult<Expr> {
        self.enter()?;
        let start = self.expect(Tok::LBrace)?;
        let inner = ty.and_then(Ty::elem);
        let mut elems = Vec::new();
        while self.peek() != &Tok::RBrace {
            elems.push(if self.peek() == &Tok::LBrace {
                self.array_lit(inner.filter(|t| matches!(t, Ty::Array(_))))?
            } else {
                self.expr()?
            });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let end = self.expect(Tok::RBrace)?;
        self.depth -= 1;
        Ok(Expr {
            kind: ExprKind::ArrayLit { ty, elems },
            span: start.to(end),
        })
    }

    fn enter(&mut self) -> PResult<()> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Diagnostic::new(
                Code::TooDeep,
                self.span(),
                msg::too_deep(MAX_DEPTH),
            ));
        }
        Ok(())
    }

    // `import a.b.c;` or `import a.b.*;`: at least a module and an item.
    fn import(&mut self) -> PResult<Import> {
        let start = self.bump().span;
        let mut module = vec![self.ident()?];
        let mut item = None;
        let mut wildcard = false;
        while self.eat(&Tok::Dot) {
            if self.peek() == &Tok::Star {
                self.bump();
                wildcard = true;
                break;
            }
            module.push(self.ident()?);
        }
        let end = self.expect(Tok::Semi)?;
        let span = start.to(end);
        if !wildcard {
            if module.len() < 2 {
                return Err(Diagnostic::new(
                    Code::BadImportPath,
                    span,
                    msg::bad_import_path(),
                ));
            }
            item = module.pop();
        }
        Ok(Import { module, item, span })
    }

    // `struct Name<A, B> implements I { Type field; ... Ret method(...) { ... } ... }`: fields
    // and methods, in any order.
    fn struct_decl(&mut self) -> PResult<StructDecl> {
        self.bump();
        let name = self.name()?;
        let params = self.type_params()?;
        self.tparams = params.iter().map(|p| p.name.clone()).collect();
        let implements = self.implements()?;
        self.expect(Tok::LBrace)?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while self.peek() != &Tok::RBrace {
            let (ty, ty_span) = self.ty()?;
            let name = self.name()?;
            if self.peek() == &Tok::LParen {
                methods.push(self.method(ty, ty_span, name)?);
                continue;
            }
            self.expect(Tok::Semi)?;
            fields.push(Field { ty, ty_span, name });
        }
        self.expect(Tok::RBrace)?;
        self.tparams.clear();
        Ok(StructDecl {
            name,
            params,
            implements,
            fields,
            methods,
        })
    }

    // `enum Name { A, B(Type x, Type y), ... }` (a trailing comma is allowed); methods follow the
    // variants after a `;`: `enum Name implements I { A, B; Ret method() { ... } }`.
    fn enum_decl(&mut self) -> PResult<EnumDecl> {
        self.bump();
        let name = self.name()?;
        let params = self.type_params()?;
        self.tparams = params.iter().map(|p| p.name.clone()).collect();
        let implements = self.implements()?;
        self.expect(Tok::LBrace)?;
        let mut variants = Vec::new();
        while !matches!(self.peek(), Tok::RBrace | Tok::Semi) {
            let vname = self.name()?;
            let mut fields = Vec::new();
            if self.eat(&Tok::LParen) {
                while self.peek() != &Tok::RParen {
                    let (ty, ty_span) = self.ty()?;
                    let name = self.name()?;
                    fields.push(Field { ty, ty_span, name });
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RParen)?;
            }
            variants.push(Variant {
                name: vname,
                fields,
            });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        // An enum needs a first variant: it is the default value.
        if variants.is_empty() {
            return Err(self.unexpected(msg::expected_ident(&self.peek().describe())));
        }
        let mut methods = Vec::new();
        if self.eat(&Tok::Semi) {
            while self.peek() != &Tok::RBrace {
                let (ty, ty_span) = self.ty()?;
                let name = self.name()?;
                methods.push(self.method(ty, ty_span, name)?);
            }
        }
        self.expect(Tok::RBrace)?;
        self.tparams.clear();
        Ok(EnumDecl {
            name,
            params,
            implements,
            variants,
            methods,
            prelude: false,
        })
    }

    // A built-in type name, a type parameter in scope, or a struct, enum or interface of this
    // file.
    fn named_ty(&self, name: &str) -> Option<Ty> {
        if self.tparams.iter().any(|p| p == name) {
            return Some(Ty::param_named(name));
        }
        Ty::from_name(name)
            .or_else(|| self.structs.contains(name).then(|| Ty::struct_named(name)))
            .or_else(|| self.enums.contains(name).then(|| Ty::enum_named(name)))
            .or_else(|| self.ifaces.contains(name).then(|| Ty::iface_named(name)))
            .or_else(|| native::TYPES.contains(&name).then(|| Ty::native(name)))
    }

    fn is_type_name(&self, name: &str) -> bool {
        Ty::from_name(name).is_some()
            || self.structs.contains(name)
            || self.enums.contains(name)
            || self.ifaces.contains(name)
            || self.tparams.iter().any(|p| p == name)
            || native::TYPES.contains(&name)
    }

    // `match e { pattern => { ... } ... }` (a comma after an arm is allowed).
    fn match_stmt(&mut self) -> PResult<Stmt> {
        self.enter()?;
        let span = self.bump().span;
        let scrutinee = self.expr()?;
        self.expect(Tok::LBrace)?;
        let mut arms = Vec::new();
        while self.peek() != &Tok::RBrace {
            let pat = self.pattern()?;
            self.expect(Tok::FatArrow)?;
            let body = self.block()?;
            arms.push(Arm { pat, body });
            self.eat(&Tok::Comma);
        }
        self.expect(Tok::RBrace)?;
        self.depth -= 1;
        Ok(Stmt::Match {
            scrutinee,
            arms,
            span,
        })
    }

    // `_`, a literal (`-1`, `"a"`, `true`), or a variant with bindings: `Shape.Rect(w, _)`.
    fn pattern(&mut self) -> PResult<Pattern> {
        let start = self.span();
        let kind =
            match self.peek().clone() {
                Tok::Ident(n) if n == "_" => {
                    self.bump();
                    PatKind::Wild
                }
                Tok::Int(n) => {
                    self.bump();
                    PatKind::Int(n)
                }
                Tok::Minus if matches!(self.peek_at(1), Tok::Int(_)) => {
                    self.bump();
                    let Tok::Int(n) = self.bump().tok else {
                        unreachable!("checked above")
                    };
                    PatKind::Int(-n)
                }
                Tok::True | Tok::False => PatKind::Bool(self.bump().tok == Tok::True),
                Tok::Str(text) => {
                    self.bump();
                    PatKind::Str(parse_format(&text).ok_or_else(|| {
                        Diagnostic::new(Code::BadFormat, start, msg::bad_format())
                    })?)
                }
                Tok::Ident(_) => {
                    let first = self.ident()?;
                    let (qual, name) = if self.eat(&Tok::Dot) {
                        (Some(first), self.ident()?)
                    } else {
                        (None, first)
                    };
                    let mut binds = Vec::new();
                    if self.eat(&Tok::LParen) {
                        while self.peek() != &Tok::RParen {
                            binds.push(self.name()?);
                            if !self.eat(&Tok::Comma) {
                                break;
                            }
                        }
                        self.expect(Tok::RParen)?;
                    }
                    PatKind::Variant { qual, name, binds }
                }
                t => return Err(self.unexpected(msg::expected_pattern(&t.describe()))),
            };
        Ok(Pattern {
            kind,
            span: start.to(self.prev_span()),
        })
    }

    // `[public] [<T, U>] ReturnType name(Type a, consume Type b) { ... }`
    fn fn_decl(&mut self) -> PResult<FnDecl> {
        let public = if self.peek() == &Tok::Public {
            Some(self.bump().span)
        } else {
            None
        };
        let (tparams, bounds) = self.bounded_type_params()?;
        let (ret, ret_span) = self.ty()?;
        let name = self.name()?;
        let params = self.params()?;
        let body = self.block()?;
        self.tparams.clear();
        Ok(FnDecl {
            public,
            tparams,
            bounds,
            name,
            params,
            ret,
            ret_span,
            body,
            method: None,
        })
    }

    fn block(&mut self) -> PResult<Block> {
        self.enter()?;
        self.expect(Tok::LBrace)?;
        let mut stmts = Vec::new();
        loop {
            match self.peek() {
                Tok::RBrace => break,
                Tok::Final | Tok::Var => stmts.push(self.decl()?),
                // `Type name = ...`: two identifiers in a row start a declaration, as does
                // `Type[]`.
                Tok::Ident(n)
                    if matches!(self.peek_at(1), Tok::Ident(_))
                        || (self.peek_at(1) == &Tok::LBracket
                            && self.peek_at(2) == &Tok::RBracket)
                        || (self.peek_at(1) == &Tok::Lt
                            && self.generics.contains(n)
                            && !self.tparams.contains(n)) =>
                {
                    stmts.push(self.decl()?)
                }
                Tok::If => stmts.push(Stmt::If(self.if_stmt()?)),
                Tok::Match => stmts.push(self.match_stmt()?),
                Tok::For => stmts.extend(self.for_stmt()?),
                Tok::While => {
                    let span = self.bump().span;
                    let cond = self.expr()?;
                    let body = self.block()?;
                    stmts.push(Stmt::While { cond, body, span });
                }
                Tok::Break | Tok::Continue => {
                    let t = self.bump();
                    let span = t.span.to(self.expect(Tok::Semi)?);
                    stmts.push(if t.tok == Tok::Break {
                        Stmt::Break { span }
                    } else {
                        Stmt::Continue { span }
                    });
                }
                Tok::Return => {
                    let start = self.bump().span;
                    let value = if self.peek() == &Tok::Semi {
                        None
                    } else {
                        Some(self.expr()?)
                    };
                    let end = self.expect(Tok::Semi)?;
                    stmts.push(Stmt::Return {
                        value,
                        span: start.to(end),
                    });
                }
                Tok::Ident(_) if self.peek_at(1) == &Tok::Eq => {
                    let name = self.ident()?;
                    self.bump();
                    let value = self.expr()?;
                    self.expect(Tok::Semi)?;
                    stmts.push(Stmt::Assign { name, value });
                }
                // `x += e` is `x = x + e` (the target is evaluated as a variable read).
                Tok::Ident(_) if compound_op(self.peek_at(1)).is_some() => {
                    let name = self.ident()?;
                    let op = compound_op(&self.bump().tok).expect("checked above");
                    let rhs = self.expr()?;
                    self.expect(Tok::Semi)?;
                    let lhs = Expr {
                        kind: ExprKind::Var(name.name.clone()),
                        span: name.span,
                    };
                    let value = Expr {
                        span: name.span.to(rhs.span),
                        kind: ExprKind::Binary {
                            op,
                            lhs: Box::new(lhs),
                            rhs: Box::new(rhs),
                        },
                    };
                    stmts.push(Stmt::Assign { name, value });
                }
                _ => {
                    let e = self.expr()?;
                    // `a[i] = v;` and `a[i] += v;`
                    let op = match self.peek() {
                        Tok::Eq => Some(None),
                        t => compound_op(t).map(Some),
                    };
                    match (op, e.kind) {
                        (Some(op), ExprKind::Field { recv, name }) => {
                            self.bump();
                            let value = self.expr()?;
                            let end = self.expect(Tok::Semi)?;
                            stmts.push(Stmt::SetField {
                                base: *recv,
                                name,
                                op,
                                value,
                                span: e.span.to(end),
                            });
                        }
                        (Some(op), ExprKind::Index { base, index }) => {
                            self.bump();
                            let value = self.expr()?;
                            let end = self.expect(Tok::Semi)?;
                            stmts.push(Stmt::SetIndex {
                                base: *base,
                                index: *index,
                                op,
                                value,
                                span: e.span.to(end),
                            });
                        }
                        (_, kind) => {
                            self.expect(Tok::Semi)?;
                            stmts.push(Stmt::Expr(Expr { kind, span: e.span }));
                        }
                    }
                }
            }
        }
        let close = self.expect(Tok::RBrace)?;
        self.depth -= 1;
        Ok(Block { stmts, close })
    }

    // `[final] Type name = init;` or `[final] var name = init;`
    fn decl(&mut self) -> PResult<Stmt> {
        let is_final = self.eat(&Tok::Final);
        let (ty, ty_span) = if self.peek() == &Tok::Var {
            (None, self.bump().span)
        } else {
            let (t, s) = self.ty()?;
            (Some(t), s)
        };
        let name = self.name()?;
        self.expect(Tok::Eq)?;
        // `int[] a = {1, 2};`: an initializer takes the declared array type.
        let init = if self.peek() == &Tok::LBrace {
            self.array_lit(ty.filter(|t| matches!(t, Ty::Array(_))))?
        } else {
            self.expr()?
        };
        self.expect(Tok::Semi)?;
        Ok(Stmt::Decl {
            is_final,
            ty,
            ty_span,
            name,
            init,
        })
    }

    fn if_stmt(&mut self) -> PResult<IfStmt> {
        self.enter()?;
        self.bump();
        let cond = self.expr()?;
        let then = self.block()?;
        let els = if self.eat(&Tok::Else) {
            if self.peek() == &Tok::If {
                Some(Else::If(Box::new(self.if_stmt()?)))
            } else {
                Some(Else::Block(self.block()?))
            }
        } else {
            None
        };
        self.depth -= 1;
        Ok(IfStmt { cond, then, els })
    }

    // `for i in a..b { }`, or `for x in xs { }` over the elements of an array or `List`, which is
    // read as `var #it = xs; for #i in 0..len(#it) { var x = #it[#i]; ... }` (hidden names no
    // program can write): the collection is evaluated once, and so is its length.
    fn for_stmt(&mut self) -> PResult<Vec<Stmt>> {
        self.bump();
        let var = self.name()?;
        self.expect(Tok::In)?;
        let start = self.expr()?;
        if self.peek() != &Tok::DotDot {
            let mut body = self.block()?;
            let at = start.span;
            let n = self.fresh;
            self.fresh += 1;
            let it = format!("#it{n}");
            let idx = format!("#i{n}");
            let var_expr = |name: &str| Expr {
                kind: ExprKind::Var(name.to_string()),
                span: at,
            };
            let ident = |name: &str| Ident {
                name: name.to_string(),
                span: at,
            };
            let elem = Stmt::Decl {
                is_final: false,
                ty: None,
                ty_span: var.span,
                name: var,
                init: Expr {
                    kind: ExprKind::Index {
                        base: Box::new(var_expr(&it)),
                        index: Box::new(var_expr(&idx)),
                    },
                    span: at,
                },
            };
            body.stmts.insert(0, elem);
            let len = Expr {
                kind: ExprKind::Call {
                    name: ident("len"),
                    args: vec![var_expr(&it)],
                },
                span: at,
            };
            return Ok(vec![
                Stmt::Decl {
                    is_final: true,
                    ty: None,
                    ty_span: at,
                    name: ident(&it),
                    init: start,
                },
                Stmt::For {
                    var: ident(&idx),
                    start: Expr {
                        kind: ExprKind::Int(0),
                        span: at,
                    },
                    end: len,
                    body,
                },
            ]);
        }
        self.expect(Tok::DotDot)?;
        let end = self.expr()?;
        let body = self.block()?;
        Ok(vec![Stmt::For {
            var,
            start,
            end,
            body,
        }])
    }

    pub fn expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let e = self.binary(0);
        self.depth -= 1;
        e
    }

    // `>>` and `>>>` are adjacent `>` tokens (they also close nested type arguments): the number
    // of `>` written together from the current token, at most 3.
    fn gt_run(&self) -> usize {
        let mut n = 0;
        while n < 3 && self.peek_at(n) == &Tok::Gt {
            let i = (self.pos + n).min(self.toks.len() - 1);
            if n > 0 && self.toks[i - 1].span.end != self.toks[i].span.start {
                break;
            }
            n += 1;
        }
        n
    }

    // Precedence climbing, loosest first: `||`, `&&`, comparisons, `|`, `^`, `&`, shifts,
    // `+ -`, `* / %`. Bitwise operators bind tighter than comparisons (as in Rust, unlike Java),
    // so `x & 1 == 0` compares the masked value.
    fn binary(&mut self, level: usize) -> PResult<Expr> {
        const LEVELS: usize = 9;
        if level == LEVELS {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        // Every operator in a chain nests the tree one level deeper, which the checker and the
        // interpreter recurse through, so it counts against the nesting limit too.
        let mut grown = 0;
        loop {
            let op = match (level, self.peek()) {
                (0, Tok::OrOr) => BinOp::Or,
                (1, Tok::AndAnd) => BinOp::And,
                (2, Tok::EqEq) => BinOp::Eq,
                (2, Tok::NotEq) => BinOp::Ne,
                (2, Tok::Lt) => BinOp::Lt,
                (2, Tok::Le) => BinOp::Le,
                (2, Tok::Gt) if self.gt_run() == 1 => BinOp::Gt,
                (2, Tok::Ge) => BinOp::Ge,
                (3, Tok::Pipe) => BinOp::BitOr,
                (4, Tok::Caret) => BinOp::BitXor,
                (5, Tok::Amp) => BinOp::BitAnd,
                (6, Tok::Shl) => BinOp::Shl,
                (6, Tok::Gt) if self.gt_run() == 2 => BinOp::Shr,
                (6, Tok::Gt) if self.gt_run() == 3 => BinOp::UShr,
                (7, Tok::Plus) => BinOp::Add,
                (7, Tok::Minus) => BinOp::Sub,
                (8, Tok::Star) => BinOp::Mul,
                (8, Tok::Slash) => BinOp::Div,
                (8, Tok::Percent) => BinOp::Rem,
                _ => {
                    self.depth -= grown;
                    return Ok(lhs);
                }
            };
            self.enter()?;
            grown += 1;
            let width = match op {
                BinOp::Shr => 2,
                BinOp::UShr => 3,
                _ => 1,
            };
            for _ in 0..width {
                self.bump();
            }
            let rhs = self.binary(level + 1)?;
            let span = lhs.span.to(rhs.span);
            lhs = Expr {
                kind: ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            };
            // Comparisons are non-associative: `a < b < c` is a syntax error.
            if level == 2 {
                if let Some(sym) = self.peek().symbol() {
                    if matches!(sym, "==" | "!=" | "<" | "<=" | ">" | ">=") {
                        return Err(self
                            .unexpected(msg::chained_comparison())
                            .with_help(msg::help_chained_comparison()));
                    }
                }
                self.depth -= grown;
                return Ok(lhs);
            }
        }
    }

    // `(Type) expr`: binds like a unary operator, as in Java.
    fn is_cast(&self) -> bool {
        self.peek() == &Tok::LParen
            && matches!(self.peek_at(1), Tok::Ident(n) if Ty::from_name(n).is_some())
            && self.peek_at(2) == &Tok::RParen
    }

    fn unary(&mut self) -> PResult<Expr> {
        if self.is_cast() {
            self.enter()?;
            let start = self.bump().span;
            let (ty, _) = self.ty()?;
            self.bump();
            let expr = self.unary()?;
            self.depth -= 1;
            let span = start.to(expr.span);
            return Ok(Expr {
                kind: ExprKind::Cast {
                    ty,
                    expr: Box::new(expr),
                },
                span,
            });
        }
        let op = match self.peek() {
            Tok::Minus => UnOp::Neg,
            Tok::Bang => UnOp::Not,
            Tok::Tilde => UnOp::BitNot,
            _ => return self.postfix(),
        };
        self.enter()?;
        let start = self.bump().span;
        let expr = self.unary()?;
        self.depth -= 1;
        let span = start.to(expr.span);
        Ok(Expr {
            kind: ExprKind::Unary {
                op,
                expr: Box::new(expr),
            },
            span,
        })
    }

    fn postfix(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        // Like operator chains, each `[i]` or `.m()` nests the tree one level deeper.
        let mut grown = 0;
        loop {
            if matches!(self.peek(), Tok::LBracket | Tok::Dot) {
                self.enter()?;
                grown += 1;
            }
            if self.eat(&Tok::LBracket) {
                let index = self.expr()?;
                let end = self.expect(Tok::RBracket)?;
                let span = e.span.to(end);
                e = Expr {
                    kind: ExprKind::Index {
                        base: Box::new(e),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.eat(&Tok::Dot) {
                // `r.asInt()`, or a call through a module alias: `fs.readString(path)`.
                let name = self.ident()?;
                // `p.x`: a field.
                if self.peek() != &Tok::LParen {
                    let span = e.span.to(name.span);
                    e = Expr {
                        kind: ExprKind::Field {
                            recv: Box::new(e),
                            name,
                        },
                        span,
                    };
                    continue;
                }
                self.expect(Tok::LParen)?;
                let (args, end) = self.call_args()?;
                let span = e.span.to(end);
                e = Expr {
                    kind: ExprKind::Method {
                        recv: Box::new(e),
                        name,
                        args,
                    },
                    span,
                };
            } else if self.peek() == &Tok::Question {
                let op = self.bump().span;
                let span = e.span.to(op);
                e = Expr {
                    kind: ExprKind::Try {
                        expr: Box::new(e),
                        op,
                    },
                    span,
                };
                self.enter()?;
                grown += 1;
            } else {
                self.depth -= grown;
                return Ok(e);
            }
        }
    }

    fn primary(&mut self) -> PResult<Expr> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => {
                self.bump();
                ExprKind::Int(n)
            }
            Tok::Double(bits) => {
                self.bump();
                ExprKind::Double(f64::from_bits(bits))
            }
            Tok::True => {
                self.bump();
                ExprKind::Bool(true)
            }
            Tok::False => {
                self.bump();
                ExprKind::Bool(false)
            }
            Tok::Str(s) => {
                self.bump();
                ExprKind::Str(
                    parse_format(&s)
                        .ok_or_else(|| Diagnostic::new(Code::BadFormat, span, msg::bad_format()))?,
                )
            }
            Tok::LParen => {
                self.bump();
                let e = self.expr()?;
                let end = self.expect(Tok::RParen)?;
                return Ok(Expr {
                    kind: e.kind,
                    span: span.to(end),
                });
            }
            // `new` is contextual (not a keyword): only `new` followed by a type name creates
            // an array, so variables and functions named `new` keep working.
            Tok::Ident(name)
                if name == "new"
                    && matches!(self.peek_at(1), Tok::Ident(t) if self.is_type_name(t)) =>
            {
                self.bump();
                return self.new_array(span);
            }
            // `Shape.Circle(2.0)` or `Shape.Empty`: a variant of an enum of this file;
            // `Option<int>.None` names the instance of a generic enum.
            Tok::Ident(name)
                if self.enums.contains(&name)
                    && !self.tparams.contains(&name)
                    && ((self.peek_at(1) == &Tok::Dot
                        && matches!(self.peek_at(2), Tok::Ident(_)))
                        || (self.peek_at(1) == &Tok::Lt && self.generics.contains(&name))) =>
            {
                self.bump();
                let mut ty = Ty::enum_named(&name);
                if self.peek() == &Tok::Lt {
                    let (args, _) = self.type_args()?;
                    ty = Ty::instance(ty, &args);
                }
                self.expect(Tok::Dot)?;
                let vname = self.ident()?;
                let (args, end) = if self.eat(&Tok::LParen) {
                    self.call_args()?
                } else {
                    (Vec::new(), vname.span)
                };
                return Ok(Expr {
                    kind: ExprKind::Variant {
                        ty,
                        name: vname,
                        args,
                    },
                    span: span.to(end),
                });
            }
            Tok::Ident(name) => {
                let name = Ident {
                    name,
                    span: self.bump().span,
                };
                if self.peek() != &Tok::LParen {
                    return Ok(Expr {
                        kind: ExprKind::Var(name.name),
                        span,
                    });
                }
                self.bump();
                let (args, end) = self.call_args()?;
                return Ok(Expr {
                    kind: ExprKind::Call { name, args },
                    span: span.to(end),
                });
            }
            t => return Err(self.unexpected(msg::expected_expr(&t.describe()))),
        };
        Ok(Expr {
            kind,
            span: span.to(self.prev_span()),
        })
    }

    // Comma-separated arguments after `(`, through the closing `)`; returns the `)` span.
    fn call_args(&mut self) -> PResult<(Vec<Expr>, Span)> {
        let mut args = Vec::new();
        while self.peek() != &Tok::RParen {
            args.push(self.expr()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let end = self.expect(Tok::RParen)?;
        Ok((args, end))
    }
}

/// Splits a decoded string literal into text and `{}` holes; `{{`/`}}` are literal braces.
fn array_depth(mut t: Ty) -> usize {
    let mut n = 0;
    while let Some(e) = t.elem() {
        n += 1;
        t = e;
    }
    n
}

fn compound_op(t: &Tok) -> Option<BinOp> {
    Some(match t {
        Tok::PlusEq => BinOp::Add,
        Tok::MinusEq => BinOp::Sub,
        Tok::StarEq => BinOp::Mul,
        Tok::SlashEq => BinOp::Div,
        Tok::PercentEq => BinOp::Rem,
        Tok::AmpEq => BinOp::BitAnd,
        Tok::PipeEq => BinOp::BitOr,
        Tok::CaretEq => BinOp::BitXor,
        Tok::ShlEq => BinOp::Shl,
        Tok::ShrEq => BinOp::Shr,
        Tok::UShrEq => BinOp::UShr,
        _ => return None,
    })
}

pub fn parse_format(s: &str) -> Option<Vec<FmtPiece>> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                lit.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                lit.push('}');
            }
            '{' if chars.peek() == Some(&'}') => {
                chars.next();
                if !lit.is_empty() {
                    out.push(FmtPiece::Lit(std::mem::take(&mut lit)));
                }
                out.push(FmtPiece::Hole);
            }
            '{' | '}' => return None,
            _ => lit.push(c),
        }
    }
    if !lit.is_empty() {
        out.push(FmtPiece::Lit(lit));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::super::lexer::lex;
    use super::*;

    fn parse_src(src: &str) -> PResult<Program> {
        parse(lex(src)?)
    }

    #[test]
    fn parses_precedence_and_statements() {
        let p = parse_src(
            "boolean f(int n) { int x = 1 + 2 * 3; x = -x; for i in 0..n { if x < 0 || i == 1 && true { x = x % 2; } else if false {} else {} } return n != x; }",
        )
        .unwrap();
        let f = &p.fns[0];
        assert_eq!(f.ret, Ty::Boolean);
        assert_eq!(f.params[0].ty, Ty::Int);
        assert_eq!(f.body.stmts.len(), 4);
        let Stmt::Decl {
            init, is_final, ty, ..
        } = &f.body.stmts[0]
        else {
            panic!()
        };
        assert!(!is_final);
        assert_eq!(*ty, Some(Ty::Int));
        let ExprKind::Binary { op, rhs, .. } = &init.kind else {
            panic!()
        };
        assert_eq!(*op, BinOp::Add);
        assert!(matches!(rhs.kind, ExprKind::Binary { op: BinOp::Mul, .. }));
    }

    #[test]
    fn java_style_declarations_and_methods() {
        let p = parse_src(
            "void main() { final double t = 0.5; Qubit q = qubit(); BitReg m = measure(qreg(2)); println(\"{}\", m.asInt()); }",
        )
        .unwrap();
        let f = &p.fns[0];
        assert_eq!(f.ret, Ty::Void);
        assert!(matches!(
            f.body.stmts[0],
            Stmt::Decl {
                is_final: true,
                ty: Some(Ty::Double),
                ..
            }
        ));
        assert!(matches!(
            f.body.stmts[1],
            Stmt::Decl {
                ty: Some(Ty::Qubit),
                ..
            }
        ));
        let Stmt::Expr(call) = &f.body.stmts[3] else {
            panic!()
        };
        let ExprKind::Call { args, .. } = &call.kind else {
            panic!()
        };
        assert!(matches!(&args[1].kind, ExprKind::Method { name, .. } if name.name == "asInt"));
    }

    #[test]
    fn no_tail_expressions() {
        let e = parse_src("int f() { 1 + 2 }").unwrap_err();
        assert_eq!(e.code, Code::UnexpectedToken);
    }

    #[test]
    fn syntax_errors_have_codes() {
        assert_eq!(
            parse_src("void main() { int x = ; }").unwrap_err().code,
            Code::UnexpectedToken
        );
        assert_eq!(
            parse_src("void main() { long x = 1; }").unwrap_err().code,
            Code::UnknownType
        );
        assert_eq!(
            parse_src("void main() { int x; }").unwrap_err().code,
            Code::UnexpectedToken
        );
        assert_eq!(
            parse_src("void main() { a < b < c; }").unwrap_err().code,
            Code::UnexpectedToken
        );
        assert_eq!(
            parse_src(r#"void main() { println("{x}"); }"#)
                .unwrap_err()
                .code,
            Code::BadFormat
        );
        let deep = format!(
            "void main() {{ int x = {}1{}; }}",
            "(".repeat(500),
            ")".repeat(500)
        );
        assert_eq!(parse_src(&deep).unwrap_err().code, Code::TooDeep);
        let neg = format!("void main() {{ int x = {}1; }}", "-".repeat(500));
        assert_eq!(parse_src(&neg).unwrap_err().code, Code::TooDeep);
    }

    #[test]
    fn format_strings() {
        assert_eq!(
            parse_format("a={} {{b}}").unwrap(),
            vec![
                FmtPiece::Lit("a=".into()),
                FmtPiece::Hole,
                FmtPiece::Lit(" {b}".into())
            ]
        );
        assert!(parse_format("{").is_none());
        assert!(parse_format("}").is_none());
    }
}

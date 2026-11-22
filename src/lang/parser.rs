// Recursive-descent parser; stops at the first syntax error.

use super::ast::*;
use super::diag::{Code, Diagnostic, Span};
use super::lexer::{Tok, Token};
use super::msg;

// Bounds recursion on hostile input (deeply nested parentheses or blocks).
pub const MAX_DEPTH: usize = 128;

pub struct Parser {
    toks: Vec<Token>,
    pos: usize,
    depth: usize,
}

type PResult<T> = Result<T, Diagnostic>;

pub fn parse(toks: Vec<Token>) -> PResult<Program> {
    let mut p = Parser {
        toks,
        pos: 0,
        depth: 0,
    };
    let mut imports = Vec::new();
    while p.peek() == &Tok::Import {
        imports.push(p.import()?);
    }
    let mut fns = Vec::new();
    while p.peek() != &Tok::Eof {
        if p.peek() == &Tok::Import {
            return Err(Diagnostic::new(
                Code::MisplacedImport,
                p.span(),
                msg::misplaced_import(),
            ));
        }
        fns.push(p.fn_decl()?);
    }
    Ok(Program { imports, fns })
}

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
                let Some(mut t) = Ty::from_name(&name) else {
                    return Err(Diagnostic::new(
                        Code::UnknownType,
                        span,
                        msg::unknown_type(&name),
                    ));
                };
                // `int[]`, `double[][]`: each `[]` wraps the type in an array.
                let mut end = span;
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

    // `new int[n][m]`, `new int[n][]` or `new int[]{1, 2}`, after the `new`.
    fn new_array(&mut self, start: Span) -> PResult<Expr> {
        let (elem, _) = match self.peek() {
            Tok::Ident(n) if Ty::from_name(n).is_some() => self.ty()?,
            t => return Err(self.unexpected(msg::expected_type(&t.describe()))),
        };
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

    // `[public] ReturnType name(Type a, consume Type b) { ... }`
    fn fn_decl(&mut self) -> PResult<FnDecl> {
        let public = if self.peek() == &Tok::Public {
            Some(self.bump().span)
        } else {
            None
        };
        let (ret, ret_span) = self.ty()?;
        let name = self.name()?;
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
        let body = self.block()?;
        Ok(FnDecl {
            public,
            name,
            params,
            ret,
            ret_span,
            body,
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
                Tok::Ident(_)
                    if matches!(self.peek_at(1), Tok::Ident(_))
                        || (self.peek_at(1) == &Tok::LBracket
                            && self.peek_at(2) == &Tok::RBracket) =>
                {
                    stmts.push(self.decl()?)
                }
                Tok::If => stmts.push(Stmt::If(self.if_stmt()?)),
                Tok::For => stmts.push(self.for_stmt()?),
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

    fn for_stmt(&mut self) -> PResult<Stmt> {
        self.bump();
        let var = self.name()?;
        self.expect(Tok::In)?;
        let start = self.expr()?;
        self.expect(Tok::DotDot)?;
        let end = self.expr()?;
        let body = self.block()?;
        Ok(Stmt::For {
            var,
            start,
            end,
            body,
        })
    }

    pub fn expr(&mut self) -> PResult<Expr> {
        self.enter()?;
        let e = self.binary(0);
        self.depth -= 1;
        e
    }

    // Precedence climbing; level 0 is `||`, the highest binary level is `* / %`.
    fn binary(&mut self, level: usize) -> PResult<Expr> {
        const LEVELS: usize = 5;
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
                (2, Tok::Gt) => BinOp::Gt,
                (2, Tok::Ge) => BinOp::Ge,
                (3, Tok::Plus) => BinOp::Add,
                (3, Tok::Minus) => BinOp::Sub,
                (4, Tok::Star) => BinOp::Mul,
                (4, Tok::Slash) => BinOp::Div,
                (4, Tok::Percent) => BinOp::Rem,
                _ => {
                    self.depth -= grown;
                    return Ok(lhs);
                }
            };
            self.enter()?;
            grown += 1;
            self.bump();
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
                // `r.as_int()`, or a call through a module alias: `fs.read_text(path)`.
                let name = self.ident()?;
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
                    && matches!(self.peek_at(1), Tok::Ident(t) if Ty::from_name(t).is_some()) =>
            {
                self.bump();
                return self.new_array(span);
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
    use super::*;
    use crate::lang::lexer::lex;

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
            "void main() { final double t = 0.5; Qubit q = qubit(); BitReg m = measure(qreg(2)); println(\"{}\", m.as_int()); }",
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
        assert!(matches!(&args[1].kind, ExprKind::Method { name, .. } if name.name == "as_int"));
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

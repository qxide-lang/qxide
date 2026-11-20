// Untyped AST produced by the parser; the checker validates it in place.

use super::diag::Span;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    Int,
    Double,
    Boolean,
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
}

impl Ty {
    pub fn name(self) -> &'static str {
        match self {
            Ty::Int => "int",
            Ty::Double => "double",
            Ty::Boolean => "boolean",
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
    pub fns: Vec<FnDecl>,
}

#[derive(Debug, Clone)]
pub struct FnDecl {
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
    Return {
        value: Option<Expr>,
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
    Method {
        recv: Box<Expr>,
        name: Ident,
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
}

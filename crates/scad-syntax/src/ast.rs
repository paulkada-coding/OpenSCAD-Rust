//! Owned abstract syntax tree consumed by the evaluator.
//!
//! Every node keeps the source range it came from, so geometry and error
//! messages can point back to the code.

use std::rc::Rc;

use rowan::TextRange;

pub type Name = smol_str::SmolStr;
pub type Span = TextRange;

#[derive(Debug, Clone, PartialEq)]
pub struct SourceFile {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StatementKind {
    Assignment(Assignment),
    ModuleDef(Rc<ModuleDef>),
    FunctionDef(Rc<FunctionDef>),
    Include(String),
    Use(String),
    Instantiation(Instantiation),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assignment {
    pub name: Name,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModuleDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: Name,
    pub params: Vec<Param>,
    pub body: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Param {
    pub name: Name,
    pub default: Option<Expr>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Modifiers {
    /// `!` — show only this subtree.
    pub root: bool,
    /// `#` — highlight.
    pub highlight: bool,
    /// `%` — transparent background, excluded from the result.
    pub background: bool,
    /// `*` — disabled.
    pub disable: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Instantiation {
    pub modifiers: Modifiers,
    pub kind: InstantiationKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstantiationKind {
    /// `name(args) children` — also used for `for`, `let`, `echo`, `assert`
    /// and `intersection_for`, which OpenSCAD treats as modules.
    Call {
        name: Name,
        name_span: Span,
        args: Vec<Argument>,
        children: Vec<Statement>,
    },
    If {
        condition: Expr,
        then_branch: Vec<Statement>,
        else_branch: Option<Vec<Statement>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Argument {
    pub name: Option<Name>,
    pub value: Expr,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Undef,
    Bool(bool),
    Number(f64),
    String(Rc<str>),
    Variable(Name),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Ternary {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Argument>,
    },
    Index {
        target: Box<Expr>,
        index: Box<Expr>,
    },
    Member {
        target: Box<Expr>,
        member: Name,
    },
    Range {
        start: Box<Expr>,
        step: Option<Box<Expr>>,
        end: Box<Expr>,
    },
    List(Vec<ListElement>),
    Let {
        assignments: Vec<Argument>,
        body: Box<Expr>,
    },
    Assert {
        args: Vec<Argument>,
        body: Option<Box<Expr>>,
    },
    Echo {
        args: Vec<Argument>,
        body: Option<Box<Expr>>,
    },
    FunctionLiteral(Rc<FunctionLiteral>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionLiteral {
    pub params: Vec<Param>,
    pub body: Expr,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnaryOp {
    Neg,
    Plus,
    Not,
    BitNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    BitAnd,
    BitOr,
    Shl,
    Shr,
}

/// An element of a list literal, possibly a generator (list comprehension).
#[derive(Debug, Clone, PartialEq)]
pub struct ListElement {
    pub kind: ListElementKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ListElementKind {
    Expr(Expr),
    Each(Box<ListElement>),
    For {
        assignments: Vec<Argument>,
        body: Box<ListElement>,
    },
    /// C-style `for (init; condition; update)`.
    ForC {
        init: Vec<Argument>,
        condition: Expr,
        update: Vec<Argument>,
        body: Box<ListElement>,
    },
    If {
        condition: Expr,
        then_element: Box<ListElement>,
        else_element: Option<Box<ListElement>>,
    },
    Let {
        assignments: Vec<Argument>,
        body: Box<ListElement>,
    },
}

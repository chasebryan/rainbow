use std::path::{Path, PathBuf};

use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Let {
        name: String,
        ty: TypeName,
        value: Expr,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Var {
        name: String,
        ty: TypeName,
        value: Expr,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Assign {
        name: String,
        value: Expr,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Import {
        path: String,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Struct {
        name: String,
        fields: Vec<Param>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Enum {
        name: String,
        variants: Vec<EnumVariant>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Fn {
        name: String,
        params: Vec<Param>,
        return_type: TypeName,
        body: Vec<Statement>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    While {
        condition: Expr,
        body: Vec<Statement>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    For {
        name: String,
        iterable: Expr,
        body: Vec<Statement>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    If {
        condition: Expr,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    IfLet {
        pattern: IfLetPattern,
        value: Expr,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Return {
        value: Option<Expr>,
        span: Span,
        source_path: Option<PathBuf>,
    },
    Break {
        span: Span,
        source_path: Option<PathBuf>,
    },
    Continue {
        span: Span,
        source_path: Option<PathBuf>,
    },
    Expr {
        expr: Expr,
        span: Span,
        source_path: Option<PathBuf>,
    },
}

impl Statement {
    pub fn span(&self) -> Span {
        match self {
            Statement::Let { span, .. }
            | Statement::Var { span, .. }
            | Statement::Assign { span, .. }
            | Statement::Import { span, .. }
            | Statement::Struct { span, .. }
            | Statement::Enum { span, .. }
            | Statement::Fn { span, .. }
            | Statement::While { span, .. }
            | Statement::For { span, .. }
            | Statement::If { span, .. }
            | Statement::IfLet { span, .. }
            | Statement::Return { span, .. }
            | Statement::Break { span, .. }
            | Statement::Continue { span, .. }
            | Statement::Expr { span, .. } => *span,
        }
    }

    pub fn source_path(&self) -> Option<&Path> {
        match self {
            Statement::Let { source_path, .. }
            | Statement::Var { source_path, .. }
            | Statement::Assign { source_path, .. }
            | Statement::Import { source_path, .. }
            | Statement::Struct { source_path, .. }
            | Statement::Enum { source_path, .. }
            | Statement::Fn { source_path, .. }
            | Statement::While { source_path, .. }
            | Statement::For { source_path, .. }
            | Statement::If { source_path, .. }
            | Statement::IfLet { source_path, .. }
            | Statement::Return { source_path, .. }
            | Statement::Break { source_path, .. }
            | Statement::Continue { source_path, .. }
            | Statement::Expr { source_path, .. } => source_path.as_deref(),
        }
    }

    pub fn set_source_path_recursive(&mut self, path: &Path) {
        let origin = Some(path.to_path_buf());
        match self {
            Statement::Let {
                value, source_path, ..
            }
            | Statement::Var {
                value, source_path, ..
            }
            | Statement::Assign {
                value, source_path, ..
            } => {
                *source_path = origin;
                value.set_source_path_recursive(path);
            }
            Statement::Import {
                source_path: target,
                ..
            }
            | Statement::Struct {
                source_path: target,
                ..
            }
            | Statement::Enum {
                source_path: target,
                ..
            }
            | Statement::Break {
                source_path: target,
                ..
            }
            | Statement::Continue {
                source_path: target,
                ..
            } => {
                *target = origin;
            }
            Statement::Return {
                value, source_path, ..
            } => {
                *source_path = origin;
                if let Some(value) = value {
                    value.set_source_path_recursive(path);
                }
            }
            Statement::Expr {
                expr, source_path, ..
            } => {
                *source_path = origin;
                expr.set_source_path_recursive(path);
            }
            Statement::Fn {
                body, source_path, ..
            } => {
                *source_path = origin;
                for statement in body {
                    statement.set_source_path_recursive(path);
                }
            }
            Statement::While {
                condition,
                body,
                source_path,
                ..
            } => {
                *source_path = origin;
                condition.set_source_path_recursive(path);
                for statement in body {
                    statement.set_source_path_recursive(path);
                }
            }
            Statement::For {
                iterable,
                body,
                source_path,
                ..
            } => {
                *source_path = origin;
                iterable.set_source_path_recursive(path);
                for statement in body {
                    statement.set_source_path_recursive(path);
                }
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                source_path,
                ..
            } => {
                *source_path = origin;
                condition.set_source_path_recursive(path);
                for statement in then_branch {
                    statement.set_source_path_recursive(path);
                }
                for statement in else_branch {
                    statement.set_source_path_recursive(path);
                }
            }
            Statement::IfLet {
                value,
                then_branch,
                else_branch,
                source_path,
                ..
            } => {
                *source_path = origin;
                value.set_source_path_recursive(path);
                for statement in then_branch {
                    statement.set_source_path_recursive(path);
                }
                for statement in else_branch {
                    statement.set_source_path_recursive(path);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: String,
    pub ty: TypeName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnumVariant {
    pub name: String,
    pub payload: Option<TypeName>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IfLetPattern {
    Binding {
        name: String,
    },
    Variant {
        enum_name: String,
        variant: String,
        binding: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub pattern: MatchPattern,
    pub body: Vec<Statement>,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MatchPattern {
    Variant {
        enum_name: String,
        variant: String,
        binding: Option<String>,
    },
    Else,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeName {
    Infer,
    I64,
    Bool,
    Str,
    Unit,
    F64,
    Struct(String),
    Array(Box<TypeName>),
    Nullable(Box<TypeName>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Nil,
    Variable(String),
    Unary {
        op: UnaryOp,
        expr: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Call {
        callee: String,
        args: Vec<Expr>,
    },
    Pipe {
        value: Box<Expr>,
        callee: String,
        args: Vec<Expr>,
    },
    StructInit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    EnumInit {
        enum_name: String,
        variant: String,
        value: Option<Box<Expr>>,
    },
    Field {
        object: Box<Expr>,
        field: String,
    },
    Array(Vec<Expr>),
    Index {
        collection: Box<Expr>,
        index: Box<Expr>,
    },
    If {
        condition: Box<Expr>,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
    },
    IfLet {
        pattern: IfLetPattern,
        value: Box<Expr>,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
    },
    Match {
        value: Box<Expr>,
        arms: Vec<MatchArm>,
    },
}

impl Expr {
    fn set_source_path_recursive(&mut self, path: &Path) {
        match self {
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::Str(_)
            | Expr::Nil
            | Expr::Variable(_) => {}
            Expr::Unary { expr, .. } => expr.set_source_path_recursive(path),
            Expr::Binary { left, right, .. } => {
                left.set_source_path_recursive(path);
                right.set_source_path_recursive(path);
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    arg.set_source_path_recursive(path);
                }
            }
            Expr::Pipe { value, args, .. } => {
                value.set_source_path_recursive(path);
                for arg in args {
                    arg.set_source_path_recursive(path);
                }
            }
            Expr::StructInit { fields, .. } => {
                for (_, value) in fields {
                    value.set_source_path_recursive(path);
                }
            }
            Expr::EnumInit { value, .. } => {
                if let Some(value) = value {
                    value.set_source_path_recursive(path);
                }
            }
            Expr::Field { object, .. } => object.set_source_path_recursive(path),
            Expr::Array(elements) => {
                for element in elements {
                    element.set_source_path_recursive(path);
                }
            }
            Expr::Index { collection, index } => {
                collection.set_source_path_recursive(path);
                index.set_source_path_recursive(path);
            }
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                condition.set_source_path_recursive(path);
                for statement in then_branch {
                    statement.set_source_path_recursive(path);
                }
                for statement in else_branch {
                    statement.set_source_path_recursive(path);
                }
            }
            Expr::IfLet {
                value,
                then_branch,
                else_branch,
                ..
            } => {
                value.set_source_path_recursive(path);
                for statement in then_branch {
                    statement.set_source_path_recursive(path);
                }
                for statement in else_branch {
                    statement.set_source_path_recursive(path);
                }
            }
            Expr::Match { value, arms } => {
                value.set_source_path_recursive(path);
                for arm in arms {
                    for statement in &mut arm.body {
                        statement.set_source_path_recursive(path);
                    }
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Coalesce,
    And,
    Or,
}

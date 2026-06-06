use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use crate::ast::{BinaryOp, Expr, Param, Program, Statement, TypeName, UnaryOp};
use crate::diagnostic::{FyrError, FyrResult};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Infer,
    I64,
    Bool,
    Str,
    Unit,
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Infer => write!(f, "infer"),
            Type::I64 => write!(f, "i64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Unit => write!(f, "unit"),
            Type::Function {
                params,
                return_type,
            } => {
                let params = params
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "fn({params}) -> {return_type}")
            }
        }
    }
}

pub fn check(program: &Program) -> FyrResult<()> {
    Checker::new().check_program(program)
}

struct Checker {
    scopes: Vec<HashMap<String, Binding>>,
}

#[derive(Debug, Clone)]
struct Binding {
    ty: Type,
    mutable: bool,
}

impl Checker {
    fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    fn check_program(mut self, program: &Program) -> FyrResult<()> {
        self.predeclare_functions(&program.statements)?;

        for statement in &program.statements {
            self.check_statement(statement)?;
        }

        Ok(())
    }

    fn predeclare_functions(&mut self, statements: &[Statement]) -> FyrResult<()> {
        for statement in statements {
            if let Statement::Fn {
                name,
                params,
                return_type,
                ..
            } = statement
            {
                reject_inferred_signature(name, params, return_type)?;

                let signature = Type::Function {
                    params: params.iter().map(|param| param.ty.as_type()).collect(),
                    return_type: Box::new(return_type.as_type()),
                };

                if self.current_scope().contains_key(name) {
                    return Err(type_error(format!("binding '{name}' already exists")));
                }

                self.define(name, signature, false);
            }
        }

        Ok(())
    }

    fn check_statement(&mut self, statement: &Statement) -> FyrResult<Type> {
        match statement {
            Statement::Let { name, value } => {
                let value_type = self.check_expr(value)?;
                self.define(name, value_type, false);
                Ok(Type::Unit)
            }
            Statement::Var { name, value } => {
                let value_type = self.check_expr(value)?;
                self.define(name, value_type, true);
                Ok(Type::Unit)
            }
            Statement::Assign { name, value } => {
                let value_type = self.check_expr(value)?;
                let binding = self
                    .lookup(name)
                    .cloned()
                    .ok_or_else(|| type_error(format!("unknown binding '{name}'")))?;

                if !binding.mutable {
                    return Err(type_error(format!(
                        "cannot assign to immutable binding '{name}'"
                    )));
                }

                if binding.ty != value_type {
                    return Err(type_error(format!(
                        "assignment to '{name}' expected {}, found {value_type}",
                        binding.ty
                    )));
                }

                Ok(Type::Unit)
            }
            Statement::Fn {
                params,
                return_type,
                body,
                ..
            } => {
                reject_inferred_signature("<local>", params, return_type)?;

                self.push_scope();
                for Param { name, ty } in params {
                    self.define(name, ty.as_type(), false);
                }
                let body_type = self.check_block(body)?;
                self.pop_scope();

                let expected = return_type.as_type();
                if body_type != expected {
                    return Err(type_error(format!(
                        "function returns {body_type}, but signature says {expected}"
                    )));
                }

                Ok(Type::Unit)
            }
            Statement::While { condition, body } => {
                self.check_while(condition, body)?;
                Ok(Type::Unit)
            }
            Statement::Expr(expr) => self.check_expr(expr),
        }
    }

    fn check_expr(&mut self, expr: &Expr) -> FyrResult<Type> {
        match expr {
            Expr::Int(_) => Ok(Type::I64),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Str(_) => Ok(Type::Str),
            Expr::Variable(name) => self
                .lookup(name)
                .map(|binding| binding.ty.clone())
                .ok_or_else(|| type_error(format!("unknown binding '{name}'"))),
            Expr::Unary { op, expr } => {
                let expr_type = self.check_expr(expr)?;
                match (op, expr_type) {
                    (UnaryOp::Negate, Type::I64) => Ok(Type::I64),
                    (UnaryOp::Not, Type::Bool) => Ok(Type::Bool),
                    (UnaryOp::Negate, found) => Err(expected_type("i64", &found)),
                    (UnaryOp::Not, found) => Err(expected_type("bool", &found)),
                }
            }
            Expr::Binary { left, op, right } => self.check_binary(left, *op, right),
            Expr::Call { callee, args } => self.check_call(callee, args),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.check_if(condition, then_branch, else_branch),
        }
    }

    fn check_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> FyrResult<Type> {
        let left_type = self.check_expr(left)?;
        let right_type = self.check_expr(right)?;

        match op {
            BinaryOp::Add if left_type == Type::I64 && right_type == Type::I64 => Ok(Type::I64),
            BinaryOp::Add if left_type == Type::Str && right_type == Type::Str => Ok(Type::Str),
            BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder
                if left_type == Type::I64 && right_type == Type::I64 =>
            {
                Ok(Type::I64)
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
                if left_type == Type::I64 && right_type == Type::I64 =>
            {
                Ok(Type::Bool)
            }
            BinaryOp::Equal | BinaryOp::NotEqual if left_type == right_type => Ok(Type::Bool),
            BinaryOp::And | BinaryOp::Or if left_type == Type::Bool && right_type == Type::Bool => {
                Ok(Type::Bool)
            }
            _ => Err(type_error(format!(
                "operator '{op:?}' cannot be applied to {left_type} and {right_type}"
            ))),
        }
    }

    fn check_call(&mut self, callee: &str, args: &[Expr]) -> FyrResult<Type> {
        match callee {
            "print" => {
                if args.len() != 1 {
                    return Err(type_error("print expects exactly one argument"));
                }
                self.check_expr(&args[0])?;
                Ok(Type::Unit)
            }
            "type" => {
                if args.len() != 1 {
                    return Err(type_error("type expects exactly one argument"));
                }
                self.check_expr(&args[0])?;
                Ok(Type::Str)
            }
            _ => {
                let signature = self
                    .lookup(callee)
                    .map(|binding| binding.ty.clone())
                    .ok_or_else(|| type_error(format!("unknown function '{callee}'")))?;

                let Type::Function {
                    params,
                    return_type,
                } = signature
                else {
                    return Err(type_error(format!("'{callee}' is not a function")));
                };

                if args.len() != params.len() {
                    return Err(type_error(format!(
                        "{callee} expects {} argument(s), found {}",
                        params.len(),
                        args.len()
                    )));
                }

                for (index, (arg, expected)) in args.iter().zip(params.iter()).enumerate() {
                    let found = self.check_expr(arg)?;
                    if &found != expected {
                        return Err(type_error(format!(
                            "argument {} for {callee} expected {expected}, found {found}",
                            index + 1
                        )));
                    }
                }

                Ok(*return_type)
            }
        }
    }

    fn check_if(
        &mut self,
        condition: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
    ) -> FyrResult<Type> {
        let condition_type = self.check_expr(condition)?;
        if condition_type != Type::Bool {
            return Err(expected_type("bool", &condition_type));
        }

        let then_type = self.check_block_scoped(then_branch)?;
        let else_type = self.check_block_scoped(else_branch)?;

        if then_type != else_type {
            return Err(type_error(format!(
                "if branches must have the same type, found {then_type} and {else_type}"
            )));
        }

        Ok(then_type)
    }

    fn check_while(&mut self, condition: &Expr, body: &[Statement]) -> FyrResult<()> {
        let condition_type = self.check_expr(condition)?;
        if condition_type != Type::Bool {
            return Err(expected_type("bool", &condition_type));
        }

        self.check_block_scoped(body)?;
        Ok(())
    }

    fn check_block_scoped(&mut self, statements: &[Statement]) -> FyrResult<Type> {
        self.push_scope();
        let result = self.check_block(statements);
        self.pop_scope();
        result
    }

    fn check_block(&mut self, statements: &[Statement]) -> FyrResult<Type> {
        let mut last_type = Type::Unit;

        for statement in statements {
            last_type = self.check_statement(statement)?;
        }

        Ok(last_type)
    }

    fn define(&mut self, name: &str, ty: Type, mutable: bool) {
        self.current_scope()
            .insert(name.to_owned(), Binding { ty, mutable });
    }

    fn lookup(&self, name: &str) -> Option<&Binding> {
        self.scopes.iter().rev().find_map(|scope| scope.get(name))
    }

    fn current_scope(&mut self) -> &mut HashMap<String, Binding> {
        self.scopes.last_mut().expect("checker always has a scope")
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        debug_assert!(!self.scopes.is_empty());
    }
}

impl TypeName {
    fn as_type(&self) -> Type {
        match self {
            TypeName::Infer => Type::Infer,
            TypeName::I64 => Type::I64,
            TypeName::Bool => Type::Bool,
            TypeName::Str => Type::Str,
            TypeName::Unit => Type::Unit,
        }
    }
}

fn reject_inferred_signature(
    name: &str,
    params: &[Param],
    return_type: &TypeName,
) -> FyrResult<()> {
    for param in params {
        if param.ty == TypeName::Infer {
            return Err(type_error(format!(
                "function '{name}' parameter '{}' needs an explicit type",
                param.name
            )));
        }
    }

    if *return_type == TypeName::Infer {
        return Err(type_error(format!(
            "function '{name}' needs an explicit return type"
        )));
    }

    Ok(())
}

fn expected_type(expected: &str, found: &Type) -> FyrError {
    type_error(format!("expected {expected}, found {found}"))
}

fn type_error(message: impl Into<String>) -> FyrError {
    FyrError::new(message, Span::new(0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn typecheck(source: &str) -> FyrResult<()> {
        let tokens = lex(source)?;
        let program = parse(&tokens)?;
        check(&program)
    }

    #[test]
    fn accepts_recursive_typed_functions() {
        typecheck(
            r#"
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)

let result = fib(10)
"#,
        )
        .expect("program should typecheck");
    }

    #[test]
    fn rejects_wrong_argument_type() {
        let error = typecheck(
            r#"
fn add(a: i64, b: i64) -> i64:
    a + b

add(1, true)
"#,
        )
        .expect_err("wrong argument should fail");

        assert!(error.message.contains("argument 2"));
    }

    #[test]
    fn rejects_untyped_function_signatures() {
        let error = typecheck(
            r#"
fn add(a, b):
    a + b
"#,
        )
        .expect_err("missing annotations should fail");

        assert!(error.message.contains("explicit type"));
    }

    #[test]
    fn accepts_typed_while_mutation() {
        typecheck(
            r#"
var total = 0
var i = 1
while i <= 5:
    total = total + i
    i = i + 1

total
"#,
        )
        .expect("loop should typecheck");
    }

    #[test]
    fn rejects_assignment_to_let() {
        let error = typecheck(
            r#"
let x = 1
x = 2
"#,
        )
        .expect_err("assignment to let should fail");

        assert!(error.message.contains("immutable"));
    }

    #[test]
    fn rejects_assignment_type_changes() {
        let error = typecheck(
            r#"
var x = 1
x = "one"
"#,
        )
        .expect_err("assignment type change should fail");

        assert!(error.message.contains("expected i64"));
    }

    #[test]
    fn rejects_mismatched_if_branches() {
        let error = typecheck(
            r#"
fn choose(flag: bool) -> i64:
    if flag:
        1
    else:
        "no"
"#,
        )
        .expect_err("branch mismatch should fail");

        assert!(error.message.contains("branches"));
    }
}

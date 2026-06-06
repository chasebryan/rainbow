use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use crate::ast::{BinaryOp, Expr, Param, Program, Statement, UnaryOp};
use crate::diagnostic::{FyrError, FyrResult};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
    Str(String),
    Function(Function),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub params: Vec<Param>,
    pub body: Vec<Statement>,
}

impl Display for Value {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(value) => write!(f, "{value}"),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Str(value) => write!(f, "{value}"),
            Value::Function(_) => write!(f, "<fn>"),
            Value::Unit => write!(f, "()"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RunResult {
    pub outputs: Vec<String>,
    pub last_value: Value,
}

#[derive(Debug, Clone)]
pub struct Evaluator {
    scopes: Vec<HashMap<String, Binding>>,
    outputs: Vec<String>,
}

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            outputs: Vec::new(),
        }
    }

    pub fn run(mut self, program: &Program) -> FyrResult<RunResult> {
        self.predefine_functions(&program.statements);

        let mut last_value = Value::Unit;

        for statement in &program.statements {
            last_value = self.eval_statement(statement)?;
        }

        Ok(RunResult {
            outputs: self.outputs,
            last_value,
        })
    }

    pub fn eval_statement(&mut self, statement: &Statement) -> FyrResult<Value> {
        match statement {
            Statement::Let { name, value } => {
                let value = self.eval_expr(value)?;
                self.define(name, value, false);
                Ok(Value::Unit)
            }
            Statement::Var { name, value } => {
                let value = self.eval_expr(value)?;
                self.define(name, value, true);
                Ok(Value::Unit)
            }
            Statement::Assign { name, value } => {
                let value = self.eval_expr(value)?;
                self.assign(name, value)?;
                Ok(Value::Unit)
            }
            Statement::Fn {
                name, params, body, ..
            } => {
                self.define_function(name, params, body);
                Ok(Value::Unit)
            }
            Statement::While { condition, body } => self.eval_while(condition, body),
            Statement::Expr(expr) => self.eval_expr(expr),
        }
    }

    fn predefine_functions(&mut self, statements: &[Statement]) {
        for statement in statements {
            if let Statement::Fn {
                name, params, body, ..
            } = statement
            {
                self.define_function(name, params, body);
            }
        }
    }

    fn define_function(&mut self, name: &str, params: &[Param], body: &[Statement]) {
        self.define(
            name,
            Value::Function(Function {
                params: params.to_vec(),
                body: body.to_vec(),
            }),
            false,
        );
    }

    fn eval_expr(&mut self, expr: &Expr) -> FyrResult<Value> {
        match expr {
            Expr::Int(value) => Ok(Value::Int(*value)),
            Expr::Bool(value) => Ok(Value::Bool(*value)),
            Expr::Str(value) => Ok(Value::Str(value.clone())),
            Expr::Variable(name) => self
                .lookup(name)
                .cloned()
                .ok_or_else(|| FyrError::new(format!("unknown binding '{name}'"), Span::new(0, 0))),
            Expr::Unary { op, expr } => {
                let value = self.eval_expr(expr)?;
                self.eval_unary(*op, value)
            }
            Expr::Binary { left, op, right } => self.eval_binary(left, *op, right),
            Expr::Call { callee, args } => self.eval_call(callee, args),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.eval_if(condition, then_branch, else_branch),
        }
    }

    fn eval_unary(&self, op: UnaryOp, value: Value) -> FyrResult<Value> {
        match (op, value) {
            (UnaryOp::Negate, Value::Int(value)) => Ok(Value::Int(-value)),
            (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
            (UnaryOp::Negate, other) => type_error("integer", &other),
            (UnaryOp::Not, other) => type_error("bool", &other),
        }
    }

    fn eval_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> FyrResult<Value> {
        if op == BinaryOp::And {
            let left = self.eval_expr(left)?;
            return match left {
                Value::Bool(false) => Ok(Value::Bool(false)),
                Value::Bool(true) => self.expect_bool(right),
                other => type_error("bool", &other),
            };
        }

        if op == BinaryOp::Or {
            let left = self.eval_expr(left)?;
            return match left {
                Value::Bool(true) => Ok(Value::Bool(true)),
                Value::Bool(false) => self.expect_bool(right),
                other => type_error("bool", &other),
            };
        }

        let left = self.eval_expr(left)?;
        let right = self.eval_expr(right)?;

        match (left, op, right) {
            (Value::Int(left), BinaryOp::Add, Value::Int(right)) => Ok(Value::Int(left + right)),
            (Value::Int(left), BinaryOp::Subtract, Value::Int(right)) => {
                Ok(Value::Int(left - right))
            }
            (Value::Int(left), BinaryOp::Multiply, Value::Int(right)) => {
                Ok(Value::Int(left * right))
            }
            (Value::Int(_), BinaryOp::Divide, Value::Int(0)) => {
                Err(runtime_error("division by zero"))
            }
            (Value::Int(left), BinaryOp::Divide, Value::Int(right)) => Ok(Value::Int(left / right)),
            (Value::Int(_), BinaryOp::Remainder, Value::Int(0)) => {
                Err(runtime_error("remainder by zero"))
            }
            (Value::Int(left), BinaryOp::Remainder, Value::Int(right)) => {
                Ok(Value::Int(left % right))
            }
            (Value::Str(left), BinaryOp::Add, Value::Str(right)) => {
                Ok(Value::Str(format!("{left}{right}")))
            }
            (left, BinaryOp::Equal, right) => Ok(Value::Bool(left == right)),
            (left, BinaryOp::NotEqual, right) => Ok(Value::Bool(left != right)),
            (Value::Int(left), BinaryOp::Less, Value::Int(right)) => Ok(Value::Bool(left < right)),
            (Value::Int(left), BinaryOp::LessEqual, Value::Int(right)) => {
                Ok(Value::Bool(left <= right))
            }
            (Value::Int(left), BinaryOp::Greater, Value::Int(right)) => {
                Ok(Value::Bool(left > right))
            }
            (Value::Int(left), BinaryOp::GreaterEqual, Value::Int(right)) => {
                Ok(Value::Bool(left >= right))
            }
            (left, op, right) => Err(runtime_error(format!(
                "operator '{op:?}' cannot be applied to {left:?} and {right:?}"
            ))),
        }
    }

    fn expect_bool(&mut self, expr: &Expr) -> FyrResult<Value> {
        match self.eval_expr(expr)? {
            Value::Bool(value) => Ok(Value::Bool(value)),
            other => type_error("bool", &other),
        }
    }

    fn eval_call(&mut self, callee: &str, args: &[Expr]) -> FyrResult<Value> {
        match callee {
            "print" => {
                if args.len() != 1 {
                    return Err(runtime_error("print expects exactly one argument"));
                }
                let value = self.eval_expr(&args[0])?;
                self.outputs.push(value.to_string());
                Ok(Value::Unit)
            }
            "type" => {
                if args.len() != 1 {
                    return Err(runtime_error("type expects exactly one argument"));
                }
                Ok(Value::Str(self.eval_expr(&args[0])?.type_name().to_owned()))
            }
            other => self.eval_user_call(other, args),
        }
    }

    fn eval_user_call(&mut self, callee: &str, args: &[Expr]) -> FyrResult<Value> {
        let function = match self.lookup(callee).cloned() {
            Some(Value::Function(function)) => function,
            Some(other) => {
                return Err(runtime_error(format!(
                    "'{callee}' is {}, not a function",
                    other.type_name()
                )));
            }
            None => return Err(runtime_error(format!("unknown function '{callee}'"))),
        };

        if args.len() != function.params.len() {
            return Err(runtime_error(format!(
                "{callee} expects {} argument(s), found {}",
                function.params.len(),
                args.len()
            )));
        }

        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval_expr(arg)?);
        }

        self.push_scope();
        for (param, value) in function.params.iter().zip(values) {
            self.define(&param.name, value, false);
        }
        let result = self.eval_block(&function.body);
        self.pop_scope();
        result
    }

    fn eval_if(
        &mut self,
        condition: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
    ) -> FyrResult<Value> {
        match self.eval_expr(condition)? {
            Value::Bool(true) => self.eval_block_scoped(then_branch),
            Value::Bool(false) => self.eval_block_scoped(else_branch),
            other => type_error("bool", &other),
        }
    }

    fn eval_while(&mut self, condition: &Expr, body: &[Statement]) -> FyrResult<Value> {
        loop {
            match self.eval_expr(condition)? {
                Value::Bool(true) => {
                    self.eval_block_scoped(body)?;
                }
                Value::Bool(false) => return Ok(Value::Unit),
                other => return type_error("bool", &other),
            }
        }
    }

    fn eval_block_scoped(&mut self, statements: &[Statement]) -> FyrResult<Value> {
        self.push_scope();
        let result = self.eval_block(statements);
        self.pop_scope();
        result
    }

    fn eval_block(&mut self, statements: &[Statement]) -> FyrResult<Value> {
        let mut last_value = Value::Unit;

        for statement in statements {
            last_value = self.eval_statement(statement)?;
        }

        Ok(last_value)
    }

    pub fn take_outputs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.outputs)
    }

    fn define(&mut self, name: &str, value: Value, mutable: bool) {
        self.scopes
            .last_mut()
            .expect("evaluator always has a scope")
            .insert(name.to_owned(), Binding { value, mutable });
    }

    fn assign(&mut self, name: &str, value: Value) -> FyrResult<()> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                if !binding.mutable {
                    return Err(runtime_error(format!(
                        "cannot assign to immutable binding '{name}'"
                    )));
                }

                binding.value = value;
                return Ok(());
            }
        }

        Err(runtime_error(format!("unknown binding '{name}'")))
    }

    fn lookup(&self, name: &str) -> Option<&Value> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).map(|binding| &binding.value))
    }

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
        debug_assert!(!self.scopes.is_empty());
    }
}

impl Default for Evaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl Value {
    fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "i64",
            Value::Bool(_) => "bool",
            Value::Str(_) => "str",
            Value::Function(_) => "fn",
            Value::Unit => "unit",
        }
    }
}

fn type_error(expected: &str, actual: &Value) -> FyrResult<Value> {
    Err(runtime_error(format!(
        "expected {expected}, found {}",
        actual.type_name()
    )))
}

fn runtime_error(message: impl Into<String>) -> FyrError {
    FyrError::new(message, Span::new(0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn run(source: &str) -> FyrResult<RunResult> {
        let tokens = lex(source)?;
        let program = parse(&tokens)?;
        Evaluator::new().run(&program)
    }

    #[test]
    fn remembers_bindings() {
        let result = run("let c_speed = 3\nc_speed + 39\n").expect("program should run");

        assert_eq!(result.last_value, Value::Int(42));
    }

    #[test]
    fn supports_string_concat() {
        let result = run("\"Fy\" + \"r\"\n").expect("program should run");

        assert_eq!(result.last_value, Value::Str("Fyr".to_owned()));
    }

    #[test]
    fn rejects_division_by_zero() {
        let error = run("1 / 0\n").expect_err("division by zero should fail");

        assert!(error.message.contains("division by zero"));
    }

    #[test]
    fn calls_recursive_functions() {
        let result = run(r#"
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)

fib(10)
"#)
        .expect("recursive program should run");

        assert_eq!(result.last_value, Value::Int(55));
    }

    #[test]
    fn resolves_forward_function_calls() {
        let result = run(r#"
print(double(21))

fn double(n: i64) -> i64:
    n * 2
"#)
        .expect("forward function call should run");

        assert_eq!(result.outputs, vec!["42"]);
        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn keeps_function_locals_scoped() {
        let result = run(r#"
let x = 1
fn shadow() -> i64:
    let x = 41
    x + 1

shadow() + x
"#)
        .expect("scoped function should run");

        assert_eq!(result.last_value, Value::Int(43));
    }

    #[test]
    fn runs_while_loop_with_mutation() {
        let result = run(r#"
var total = 0
var i = 1
while i <= 5:
    total = total + i
    i = i + 1

total
"#)
        .expect("loop should run");

        assert_eq!(result.last_value, Value::Int(15));
    }
}

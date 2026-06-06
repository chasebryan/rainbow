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
    Array(Vec<Value>),
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
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
            Value::Array(values) => {
                let values = values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "[{values}]")
            }
            Value::Struct { name, .. } => write!(f, "<{name}>"),
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

const MAX_RANGE_ELEMENTS: i128 = 1_000_000;

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

#[derive(Debug, Clone, PartialEq)]
enum Flow {
    Value(Value),
    Return(Value),
    Break,
    Continue,
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
            last_value = match self.eval_statement_flow(statement)? {
                Flow::Value(value) => value,
                Flow::Return(_) => return Err(runtime_error("return outside function")),
                Flow::Break => return Err(runtime_error("break outside loop")),
                Flow::Continue => return Err(runtime_error("continue outside loop")),
            };
        }

        Ok(RunResult {
            outputs: self.outputs,
            last_value,
        })
    }

    pub fn eval_statement(&mut self, statement: &Statement) -> FyrResult<Value> {
        match self.eval_statement_flow(statement)? {
            Flow::Value(value) => Ok(value),
            Flow::Return(_) => Err(runtime_error("return outside function")),
            Flow::Break => Err(runtime_error("break outside loop")),
            Flow::Continue => Err(runtime_error("continue outside loop")),
        }
    }

    fn eval_statement_flow(&mut self, statement: &Statement) -> FyrResult<Flow> {
        match statement {
            Statement::Struct { .. } => Ok(Flow::Value(Value::Unit)),
            Statement::Let { name, value, .. } => {
                let value = self.eval_value(value)?;
                self.define(name, value, false);
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Var { name, value, .. } => {
                let value = self.eval_value(value)?;
                self.define(name, value, true);
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Assign { name, value } => {
                let value = self.eval_value(value)?;
                self.assign(name, value)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Fn {
                name, params, body, ..
            } => {
                self.define_function(name, params, body);
                Ok(Flow::Value(Value::Unit))
            }
            Statement::While { condition, body } => self.eval_while(condition, body),
            Statement::For {
                name,
                iterable,
                body,
            } => self.eval_for(name, iterable, body),
            Statement::If {
                condition,
                then_branch,
                else_branch,
            } => self.eval_if(condition, then_branch, else_branch),
            Statement::Return { value } => {
                let value = match value {
                    Some(value) => self.eval_value(value)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(value))
            }
            Statement::Break => Ok(Flow::Break),
            Statement::Continue => Ok(Flow::Continue),
            Statement::Expr(expr) => self.eval_expr_flow(expr),
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

    fn eval_value(&mut self, expr: &Expr) -> FyrResult<Value> {
        match self.eval_expr_flow(expr)? {
            Flow::Value(value) => Ok(value),
            Flow::Return(value) => Err(runtime_error(format!(
                "return produced {value} where a value expression was required"
            ))),
            Flow::Break => Err(runtime_error("break where a value expression was required")),
            Flow::Continue => Err(runtime_error(
                "continue where a value expression was required",
            )),
        }
    }

    fn eval_expr_flow(&mut self, expr: &Expr) -> FyrResult<Flow> {
        match expr {
            Expr::Int(value) => Ok(Flow::Value(Value::Int(*value))),
            Expr::Bool(value) => Ok(Flow::Value(Value::Bool(*value))),
            Expr::Str(value) => Ok(Flow::Value(Value::Str(value.clone()))),
            Expr::Variable(name) => {
                self.lookup(name).cloned().map(Flow::Value).ok_or_else(|| {
                    FyrError::new(format!("unknown binding '{name}'"), Span::new(0, 0))
                })
            }
            Expr::Unary { op, expr } => {
                let value = match self.eval_expr_flow(expr)? {
                    Flow::Value(value) => value,
                    flow => return Ok(flow),
                };
                Ok(Flow::Value(self.eval_unary(*op, value)?))
            }
            Expr::Binary { left, op, right } => self.eval_binary(left, *op, right),
            Expr::Call { callee, args } => self.eval_call(callee, args),
            Expr::StructInit { name, fields } => self.eval_struct_init(name, fields),
            Expr::Field { object, field } => self.eval_field(object, field),
            Expr::Array(elements) => self.eval_array(elements),
            Expr::Index { collection, index } => self.eval_index(collection, index),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.eval_if(condition, then_branch, else_branch),
        }
    }

    fn eval_array(&mut self, elements: &[Expr]) -> FyrResult<Flow> {
        let mut values = Vec::with_capacity(elements.len());

        for element in elements {
            match self.eval_expr_flow(element)? {
                Flow::Value(value) => values.push(value),
                flow => return Ok(flow),
            }
        }

        Ok(Flow::Value(Value::Array(values)))
    }

    fn eval_index(&mut self, collection: &Expr, index: &Expr) -> FyrResult<Flow> {
        let collection = match self.eval_expr_flow(collection)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let index = match self.eval_expr_flow(index)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        let values = match collection {
            Value::Array(values) => values,
            other => {
                return Err(runtime_error(format!(
                    "indexing expected an array, found {}",
                    other.type_name()
                )));
            }
        };
        let index = match index {
            Value::Int(index) => index,
            other => {
                return Err(runtime_error(format!(
                    "array index expected i64, found {}",
                    other.type_name()
                )));
            }
        };

        if index < 0 {
            return Err(runtime_error(format!("array index {index} out of bounds")));
        }

        values
            .get(index as usize)
            .cloned()
            .map(Flow::Value)
            .ok_or_else(|| runtime_error(format!("array index {index} out of bounds")))
    }

    fn eval_struct_init(&mut self, name: &str, fields: &[(String, Expr)]) -> FyrResult<Flow> {
        let mut values = HashMap::new();

        for (field, expr) in fields {
            match self.eval_expr_flow(expr)? {
                Flow::Value(value) => {
                    values.insert(field.clone(), value);
                }
                flow => return Ok(flow),
            }
        }

        Ok(Flow::Value(Value::Struct {
            name: name.to_owned(),
            fields: values,
        }))
    }

    fn eval_field(&mut self, object: &Expr, field: &str) -> FyrResult<Flow> {
        match self.eval_expr_flow(object)? {
            Flow::Value(Value::Struct { name, fields }) => fields
                .get(field)
                .cloned()
                .map(Flow::Value)
                .ok_or_else(|| runtime_error(format!("struct '{name}' has no field '{field}'"))),
            Flow::Value(other) => Err(runtime_error(format!(
                "field access expected a struct, found {}",
                other.type_name()
            ))),
            flow => Ok(flow),
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

    fn eval_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> FyrResult<Flow> {
        if op == BinaryOp::And {
            let left = match self.eval_expr_flow(left)? {
                Flow::Value(value) => value,
                flow => return Ok(flow),
            };
            return match left {
                Value::Bool(false) => Ok(Flow::Value(Value::Bool(false))),
                Value::Bool(true) => self.expect_bool(right),
                other => type_error("bool", &other),
            };
        }

        if op == BinaryOp::Or {
            let left = match self.eval_expr_flow(left)? {
                Flow::Value(value) => value,
                flow => return Ok(flow),
            };
            return match left {
                Value::Bool(true) => Ok(Flow::Value(Value::Bool(true))),
                Value::Bool(false) => self.expect_bool(right),
                other => type_error("bool", &other),
            };
        }

        let left = match self.eval_expr_flow(left)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let right = match self.eval_expr_flow(right)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        let value = match (left, op, right) {
            (Value::Int(left), BinaryOp::Add, Value::Int(right)) => Value::Int(left + right),
            (Value::Int(left), BinaryOp::Subtract, Value::Int(right)) => Value::Int(left - right),
            (Value::Int(left), BinaryOp::Multiply, Value::Int(right)) => Value::Int(left * right),
            (Value::Int(_), BinaryOp::Divide, Value::Int(0)) => {
                return Err(runtime_error("division by zero"));
            }
            (Value::Int(left), BinaryOp::Divide, Value::Int(right)) => Value::Int(left / right),
            (Value::Int(_), BinaryOp::Remainder, Value::Int(0)) => {
                return Err(runtime_error("remainder by zero"));
            }
            (Value::Int(left), BinaryOp::Remainder, Value::Int(right)) => Value::Int(left % right),
            (Value::Str(left), BinaryOp::Add, Value::Str(right)) => {
                Value::Str(format!("{left}{right}"))
            }
            (Value::Array(mut left), BinaryOp::Add, Value::Array(right)) => {
                left.extend(right);
                Value::Array(left)
            }
            (left, BinaryOp::Equal, right) => Value::Bool(values_equal(&left, &right)?),
            (left, BinaryOp::NotEqual, right) => Value::Bool(!values_equal(&left, &right)?),
            (Value::Int(left), BinaryOp::Less, Value::Int(right)) => Value::Bool(left < right),
            (Value::Int(left), BinaryOp::LessEqual, Value::Int(right)) => {
                Value::Bool(left <= right)
            }
            (Value::Int(left), BinaryOp::Greater, Value::Int(right)) => Value::Bool(left > right),
            (Value::Int(left), BinaryOp::GreaterEqual, Value::Int(right)) => {
                Value::Bool(left >= right)
            }
            (left, op, right) => Err(runtime_error(format!(
                "operator '{op:?}' cannot be applied to {left:?} and {right:?}"
            )))?,
        };

        Ok(Flow::Value(value))
    }

    fn expect_bool(&mut self, expr: &Expr) -> FyrResult<Flow> {
        match self.eval_expr_flow(expr)? {
            Flow::Value(Value::Bool(value)) => Ok(Flow::Value(Value::Bool(value))),
            Flow::Value(other) => type_error("bool", &other),
            flow => Ok(flow),
        }
    }

    fn eval_call(&mut self, callee: &str, args: &[Expr]) -> FyrResult<Flow> {
        match callee {
            "len" => {
                if args.len() != 1 {
                    return Err(runtime_error("len expects exactly one argument"));
                }
                let value = match self.eval_expr_flow(&args[0])? {
                    Flow::Value(value) => value,
                    flow => return Ok(flow),
                };
                match value {
                    Value::Array(values) => Ok(Flow::Value(Value::Int(values.len() as i64))),
                    Value::Str(value) => Ok(Flow::Value(Value::Int(value.chars().count() as i64))),
                    other => Err(runtime_error(format!(
                        "len expects an array or str, found {}",
                        other.type_name()
                    ))),
                }
            }
            "range" => self.eval_range(args),
            "assert" => self.eval_assert(args),
            "print" => {
                if args.len() != 1 {
                    return Err(runtime_error("print expects exactly one argument"));
                }
                let value = match self.eval_expr_flow(&args[0])? {
                    Flow::Value(value) => value,
                    flow => return Ok(flow),
                };
                self.outputs.push(value.to_string());
                Ok(Flow::Value(Value::Unit))
            }
            "type" => {
                if args.len() != 1 {
                    return Err(runtime_error("type expects exactly one argument"));
                }
                let value = match self.eval_expr_flow(&args[0])? {
                    Flow::Value(value) => value,
                    flow => return Ok(flow),
                };
                Ok(Flow::Value(Value::Str(value.type_name().to_owned())))
            }
            other => self.eval_user_call(other, args),
        }
    }

    fn eval_range(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if !(1..=2).contains(&args.len()) {
            return Err(runtime_error("range expects one or two arguments"));
        }

        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            match self.eval_expr_flow(arg)? {
                Flow::Value(Value::Int(value)) => values.push(value),
                Flow::Value(other) => {
                    return Err(runtime_error(format!(
                        "range expects i64 arguments, found {}",
                        other.type_name()
                    )));
                }
                flow => return Ok(flow),
            }
        }

        let (start, end) = if values.len() == 1 {
            (0, values[0])
        } else {
            (values[0], values[1])
        };

        if end <= start {
            return Ok(Flow::Value(Value::Array(Vec::new())));
        }

        let length = i128::from(end) - i128::from(start);
        if length > MAX_RANGE_ELEMENTS {
            return Err(runtime_error(format!(
                "range would create {length} elements; maximum is {MAX_RANGE_ELEMENTS}"
            )));
        }

        let values = (start..end).map(Value::Int).collect();
        Ok(Flow::Value(Value::Array(values)))
    }

    fn eval_assert(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if !(1..=2).contains(&args.len()) {
            return Err(runtime_error("assert expects one or two arguments"));
        }

        let condition = match self.eval_expr_flow(&args[0])? {
            Flow::Value(Value::Bool(value)) => value,
            Flow::Value(other) => {
                return Err(runtime_error(format!(
                    "assert expects a bool condition, found {}",
                    other.type_name()
                )));
            }
            flow => return Ok(flow),
        };

        let message = if let Some(message) = args.get(1) {
            match self.eval_expr_flow(message)? {
                Flow::Value(Value::Str(value)) => Some(value),
                Flow::Value(other) => {
                    return Err(runtime_error(format!(
                        "assert message expected str, found {}",
                        other.type_name()
                    )));
                }
                flow => return Ok(flow),
            }
        } else {
            None
        };

        if condition {
            Ok(Flow::Value(Value::Unit))
        } else if let Some(message) = message {
            Err(runtime_error(format!("assertion failed: {message}")))
        } else {
            Err(runtime_error("assertion failed"))
        }
    }

    fn eval_user_call(&mut self, callee: &str, args: &[Expr]) -> FyrResult<Flow> {
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
            match self.eval_expr_flow(arg)? {
                Flow::Value(value) => values.push(value),
                flow => return Ok(flow),
            }
        }

        self.push_scope();
        for (param, value) in function.params.iter().zip(values) {
            self.define(&param.name, value, false);
        }
        let result = self.eval_block(&function.body);
        self.pop_scope();
        match result? {
            Flow::Return(value) | Flow::Value(value) => Ok(Flow::Value(value)),
            Flow::Break => Err(runtime_error("break outside loop")),
            Flow::Continue => Err(runtime_error("continue outside loop")),
        }
    }

    fn eval_if(
        &mut self,
        condition: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
    ) -> FyrResult<Flow> {
        match self.eval_expr_flow(condition)? {
            Flow::Value(Value::Bool(true)) => self.eval_block_scoped(then_branch),
            Flow::Value(Value::Bool(false)) => self.eval_block_scoped(else_branch),
            Flow::Value(other) => type_error("bool", &other),
            flow => Ok(flow),
        }
    }

    fn eval_while(&mut self, condition: &Expr, body: &[Statement]) -> FyrResult<Flow> {
        loop {
            match self.eval_expr_flow(condition)? {
                Flow::Value(Value::Bool(true)) => match self.eval_block_scoped(body)? {
                    Flow::Value(_) => {}
                    Flow::Return(value) => return Ok(Flow::Return(value)),
                    Flow::Break => return Ok(Flow::Value(Value::Unit)),
                    Flow::Continue => continue,
                },
                Flow::Value(Value::Bool(false)) => return Ok(Flow::Value(Value::Unit)),
                Flow::Value(other) => return type_error("bool", &other),
                flow => return Ok(flow),
            }
        }
    }

    fn eval_for(&mut self, name: &str, iterable: &Expr, body: &[Statement]) -> FyrResult<Flow> {
        let iterable = match self.eval_expr_flow(iterable)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        let Value::Array(values) = iterable else {
            return type_error("array", &iterable);
        };

        for value in values {
            self.push_scope();
            self.define(name, value, false);
            let result = self.eval_block(body);
            self.pop_scope();

            match result? {
                Flow::Value(_) => {}
                Flow::Return(value) => return Ok(Flow::Return(value)),
                Flow::Break => return Ok(Flow::Value(Value::Unit)),
                Flow::Continue => continue,
            }
        }

        Ok(Flow::Value(Value::Unit))
    }

    fn eval_block_scoped(&mut self, statements: &[Statement]) -> FyrResult<Flow> {
        self.push_scope();
        let result = self.eval_block(statements);
        self.pop_scope();
        result
    }

    fn eval_block(&mut self, statements: &[Statement]) -> FyrResult<Flow> {
        let mut last_value = Value::Unit;

        for statement in statements {
            match self.eval_statement_flow(statement)? {
                Flow::Value(value) => last_value = value,
                flow => return Ok(flow),
            }
        }

        Ok(Flow::Value(last_value))
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
            Value::Array(_) => "array",
            Value::Struct { .. } => "struct",
            Value::Function(_) => "fn",
            Value::Unit => "unit",
        }
    }
}

fn type_error<T>(expected: &str, actual: &Value) -> FyrResult<T> {
    Err(runtime_error(format!(
        "expected {expected}, found {}",
        actual.type_name()
    )))
}

fn values_equal(left: &Value, right: &Value) -> FyrResult<bool> {
    match (left, right) {
        (Value::Function(_), _) | (_, Value::Function(_)) => {
            Err(runtime_error("functions cannot be compared for equality"))
        }
        (Value::Array(left), Value::Array(right)) => {
            if left.len() != right.len() {
                return Ok(false);
            }

            for (left, right) in left.iter().zip(right) {
                if !values_equal(left, right)? {
                    return Ok(false);
                }
            }

            Ok(true)
        }
        (
            Value::Struct {
                name: left_name,
                fields: left_fields,
            },
            Value::Struct {
                name: right_name,
                fields: right_fields,
            },
        ) => {
            if left_name != right_name || left_fields.len() != right_fields.len() {
                return Ok(false);
            }

            for (field, left) in left_fields {
                let Some(right) = right_fields.get(field) else {
                    return Ok(false);
                };
                if !values_equal(left, right)? {
                    return Ok(false);
                }
            }

            Ok(true)
        }
        _ => Ok(left == right),
    }
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

    #[test]
    fn returns_from_inside_loop() {
        let result = run(r#"
fn first_multiple_of_seven(limit: i64) -> i64:
    var i = 1
    while i <= limit:
        if i % 7 == 0:
            return i
        else:
            i = i + 1
    return -1

first_multiple_of_seven(20)
"#)
        .expect("return should leave the loop and function");

        assert_eq!(result.last_value, Value::Int(7));
    }

    #[test]
    fn supports_break_and_continue() {
        let result = run(r#"
var total = 0
var i = 0
while true:
    i = i + 1
    if i == 3:
        continue
    else:
        total = total + i
    if i >= 5:
        break
    else:
        total = total

total
"#)
        .expect("break and continue should run");

        assert_eq!(result.last_value, Value::Int(12));
    }

    #[test]
    fn constructs_structs_and_reads_fields() {
        let result = run(r#"
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
p.x * p.x + p.y * p.y
"#)
        .expect("struct program should run");

        assert_eq!(result.last_value, Value::Int(25));
    }

    #[test]
    fn sums_array_with_checked_indexing() {
        let result = run(r#"
let values = [1, 2, 3, 4]
var total = 0
var i = 0
while i < len(values):
    total = total + values[i]
    i = i + 1

total
"#)
        .expect("array program should run");

        assert_eq!(result.last_value, Value::Int(10));
    }

    #[test]
    fn evaluates_typed_empty_arrays() {
        let result =
            run("let values: [i64] = []\nlen(values)\n").expect("typed empty array should run");

        assert_eq!(result.last_value, Value::Int(0));
    }

    #[test]
    fn runs_for_loop_over_array() {
        let result = run(r#"
var total = 0
for value in [1, 2, 3, 4]:
    total = total + value

total
"#)
        .expect("for loop should run");

        assert_eq!(result.last_value, Value::Int(10));
    }

    #[test]
    fn supports_break_and_continue_in_for_loop() {
        let result = run(r#"
var total = 0
for value in [1, 2, 3, 4, 5]:
    if value == 2:
        continue
    total = total + value
    if value == 4:
        break

total
"#)
        .expect("for loop control flow should run");

        assert_eq!(result.last_value, Value::Int(8));
    }

    #[test]
    fn runs_if_statement_without_else() {
        let result = run(r#"
var total = 0
if true:
    total = 42
if false:
    total = 0

total
"#)
        .expect("if statement should run");

        assert_eq!(result.last_value, Value::Int(42));
    }

    #[test]
    fn builds_integer_ranges() {
        let result = run(r#"
var total = 0
for value in range(1, 5):
    total = total + value

total
"#)
        .expect("range loop should run");

        assert_eq!(result.last_value, Value::Int(10));
    }

    #[test]
    fn supports_single_argument_range() {
        let result = run("range(4)[3]\n").expect("single-argument range should run");

        assert_eq!(result.last_value, Value::Int(3));
    }

    #[test]
    fn rejects_large_ranges() {
        let error = run("range(0, 1000001)\n").expect_err("large range should fail");

        assert!(error.message.contains("maximum is 1000000"));
    }

    #[test]
    fn supports_assertions() {
        let result = run("assert(1 + 1 == 2)\nassert(true, \"math still works\")\n")
            .expect("assertions should pass");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn rejects_failed_assertions() {
        let error =
            run("assert(false, \"expected failure\")\n").expect_err("assertion should fail");

        assert!(error.message.contains("assertion failed: expected failure"));
    }

    #[test]
    fn compares_arrays_and_structs() {
        let result = run(r#"
struct Point:
    x: i64
    y: i64

let a = Point { x: 3, y: 4 }
let b = Point { x: 3, y: 4 }
let c = Point { x: 5, y: 12 }

assert([1, 2, 3] == [1, 2, 3])
assert([1, 2, 3] != [1, 2, 4])
assert(a == b)
assert(a != c)
"#)
        .expect("data equality should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn rejects_function_equality_at_runtime() {
        let error = run(r#"
fn id(value: i64) -> i64:
    value

id == id
"#)
        .expect_err("function equality should fail");

        assert!(error.message.contains("functions cannot be compared"));
    }

    #[test]
    fn concatenates_arrays() {
        let result = run(r#"
let left = [1, 2]
let right = [3, 4]
left + right
"#)
        .expect("array concatenation should run");

        assert_eq!(
            result.last_value,
            Value::Array(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ])
        );
    }

    #[test]
    fn concatenates_with_empty_array_literal() {
        let result = run(r#"
let left = [1, 2] + []
let right = [] + [3, 4]
left + right
"#)
        .expect("empty array concatenation should run");

        assert_eq!(
            result.last_value,
            Value::Array(vec![
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4)
            ])
        );
    }

    #[test]
    fn rejects_out_of_bounds_array_index() {
        let error = run("[1, 2][2]\n").expect_err("out-of-bounds index should fail");

        assert!(error.message.contains("out of bounds"));
    }
}

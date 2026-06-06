use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use crate::ast::{BinaryOp, Expr, Param, Program, Statement, TypeName, UnaryOp};
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
    pub return_type: TypeName,
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
    structs: HashMap<String, Vec<Param>>,
    outputs: Vec<String>,
}

const MAX_RANGE_ELEMENTS: i128 = 1_000_000;

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    ty: TypeName,
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
            structs: HashMap::new(),
            outputs: Vec::new(),
        }
    }

    pub fn run(mut self, program: &Program) -> FyrResult<RunResult> {
        self.predefine_structs(&program.statements)?;
        self.predefine_functions(&program.statements)?;

        let mut last_value = Value::Unit;

        for statement in &program.statements {
            if matches!(statement, Statement::Struct { .. } | Statement::Fn { .. }) {
                last_value = Value::Unit;
                continue;
            }

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
            Statement::Struct { name, fields } => {
                self.define_struct(name, fields)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Let { name, ty, value } => {
                let value = self.eval_value(value)?;
                self.define_binding(name, ty, value, false)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Var { name, ty, value } => {
                let value = self.eval_value(value)?;
                self.define_binding(name, ty, value, true)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Assign { name, value } => {
                let value = self.eval_value(value)?;
                self.assign(name, value)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Fn {
                name,
                params,
                return_type,
                body,
            } => {
                self.define_function(name, params, return_type, body)?;
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

    fn predefine_structs(&mut self, statements: &[Statement]) -> FyrResult<()> {
        for statement in statements {
            if let Statement::Struct { name, fields } = statement {
                self.define_struct(name, fields)?;
            }
        }

        Ok(())
    }

    fn predefine_functions(&mut self, statements: &[Statement]) -> FyrResult<()> {
        for statement in statements {
            if let Statement::Fn {
                name,
                params,
                return_type,
                body,
            } = statement
            {
                self.define_function(name, params, return_type, body)?;
            }
        }

        Ok(())
    }

    fn define_function(
        &mut self,
        name: &str,
        params: &[Param],
        return_type: &TypeName,
        body: &[Statement],
    ) -> FyrResult<()> {
        reject_inferred_signature(name, params, return_type)?;
        reject_duplicate_members("function", name, "parameter", params)?;

        self.define(
            name,
            Value::Function(Function {
                params: params.to_vec(),
                return_type: return_type.clone(),
                body: body.to_vec(),
            }),
            TypeName::Infer,
            false,
        )
    }

    fn define_struct(&mut self, name: &str, fields: &[Param]) -> FyrResult<()> {
        if self.structs.contains_key(name) || self.current_scope().contains_key(name) {
            return Err(runtime_error(format!("struct '{name}' already exists")));
        }

        let mut seen = HashMap::new();
        for field in fields {
            if seen.insert(field.name.clone(), ()).is_some() {
                return Err(runtime_error(format!(
                    "struct '{name}' has duplicate field '{}'",
                    field.name
                )));
            }
        }

        self.structs.insert(name.to_owned(), fields.to_vec());
        Ok(())
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
        let declared_fields = self
            .structs
            .get(name)
            .cloned()
            .ok_or_else(|| runtime_error(format!("unknown struct '{name}'")))?;

        let mut seen = HashMap::new();
        for (field_name, _) in fields {
            if seen.insert(field_name.clone(), ()).is_some() {
                return Err(runtime_error(format!(
                    "field '{field_name}' initialized more than once"
                )));
            }

            if !declared_fields
                .iter()
                .any(|field| field.name == *field_name)
            {
                return Err(runtime_error(format!(
                    "struct '{name}' has no field '{field_name}'"
                )));
            }
        }

        for field in &declared_fields {
            if !seen.contains_key(&field.name) {
                return Err(runtime_error(format!(
                    "struct '{name}' missing field '{}'",
                    field.name
                )));
            }
        }

        let mut values = HashMap::new();

        for (field, expr) in fields {
            let field_type = declared_fields
                .iter()
                .find(|declared| declared.name == *field)
                .expect("struct literal fields were validated")
                .ty
                .clone();
            match self.eval_expr_flow(expr)? {
                Flow::Value(value) => {
                    if !value_matches_type(&value, &field_type) {
                        return Err(runtime_error(format!(
                            "field '{field}' expected {}, found {}",
                            format_type_name(&field_type),
                            format_value_type(&value)
                        )));
                    }
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
            (UnaryOp::Negate, Value::Int(value)) => checked_int("negation", value.checked_neg()),
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
            (Value::Int(left), BinaryOp::Add, Value::Int(right)) => {
                checked_int("addition", left.checked_add(right))?
            }
            (Value::Int(left), BinaryOp::Subtract, Value::Int(right)) => {
                checked_int("subtraction", left.checked_sub(right))?
            }
            (Value::Int(left), BinaryOp::Multiply, Value::Int(right)) => {
                checked_int("multiplication", left.checked_mul(right))?
            }
            (Value::Int(_), BinaryOp::Divide, Value::Int(0)) => {
                return Err(runtime_error("division by zero"));
            }
            (Value::Int(left), BinaryOp::Divide, Value::Int(right)) => {
                checked_int("division", left.checked_div(right))?
            }
            (Value::Int(_), BinaryOp::Remainder, Value::Int(0)) => {
                return Err(runtime_error("remainder by zero"));
            }
            (Value::Int(left), BinaryOp::Remainder, Value::Int(right)) => {
                checked_int("remainder", left.checked_rem(right))?
            }
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
            "contains" => self.eval_contains(args),
            "slice" => self.eval_slice(args),
            "append" => self.eval_append(args),
            "is_empty" => self.eval_is_empty(args),
            "get" => self.eval_get(args),
            "find" => self.eval_find(args),
            "count" => self.eval_count(args),
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

    fn eval_count(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error("count expects exactly two arguments"));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let needle = match self.eval_expr_flow(&args[1])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match (collection, needle) {
            (Value::Array(values), needle) => {
                let mut count = 0;
                for value in &values {
                    if values_equal(value, &needle)? {
                        count += 1;
                    }
                }
                Ok(Flow::Value(Value::Int(count)))
            }
            (Value::Str(value), Value::Str(needle)) => {
                let count = if needle.is_empty() {
                    0
                } else {
                    value.matches(&needle).count() as i64
                };
                Ok(Flow::Value(Value::Int(count)))
            }
            (Value::Str(_), other) => Err(runtime_error(format!(
                "count(str, value) expected str, found {}",
                other.type_name()
            ))),
            (other, _) => Err(runtime_error(format!(
                "count expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_find(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error("find expects exactly two arguments"));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let needle = match self.eval_expr_flow(&args[1])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match (collection, needle) {
            (Value::Array(values), needle) => {
                for (index, value) in values.iter().enumerate() {
                    if values_equal(value, &needle)? {
                        return Ok(Flow::Value(Value::Int(index as i64)));
                    }
                }
                Ok(Flow::Value(Value::Int(-1)))
            }
            (Value::Str(value), Value::Str(needle)) => {
                let index = value
                    .find(&needle)
                    .map(|byte_index| value[..byte_index].chars().count() as i64)
                    .unwrap_or(-1);
                Ok(Flow::Value(Value::Int(index)))
            }
            (Value::Str(_), other) => Err(runtime_error(format!(
                "find(str, value) expected str, found {}",
                other.type_name()
            ))),
            (other, _) => Err(runtime_error(format!(
                "find expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_get(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 3 {
            return Err(runtime_error("get expects exactly three arguments"));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let index = match self.eval_expr_flow(&args[1])? {
            Flow::Value(Value::Int(value)) => value,
            Flow::Value(other) => {
                return Err(runtime_error(format!(
                    "get index expected i64, found {}",
                    other.type_name()
                )));
            }
            flow => return Ok(flow),
        };

        match collection {
            Value::Array(values) => match usize::try_from(index)
                .ok()
                .and_then(|index| values.get(index).cloned())
            {
                Some(value) => Ok(Flow::Value(value)),
                None => self.eval_expr_flow(&args[2]),
            },
            Value::Str(value) => match usize::try_from(index)
                .ok()
                .and_then(|index| value.chars().nth(index))
            {
                Some(ch) => Ok(Flow::Value(Value::Str(ch.to_string()))),
                None => self.eval_expr_flow(&args[2]),
            },
            other => Err(runtime_error(format!(
                "get expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_is_empty(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 1 {
            return Err(runtime_error("is_empty expects exactly one argument"));
        }

        let value = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match value {
            Value::Array(values) => Ok(Flow::Value(Value::Bool(values.is_empty()))),
            Value::Str(value) => Ok(Flow::Value(Value::Bool(value.is_empty()))),
            other => Err(runtime_error(format!(
                "is_empty expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_append(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error("append expects exactly two arguments"));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let value = match self.eval_expr_flow(&args[1])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match collection {
            Value::Array(mut values) => {
                values.push(value);
                Ok(Flow::Value(Value::Array(values)))
            }
            other => Err(runtime_error(format!(
                "append expects an array, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_slice(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 3 {
            return Err(runtime_error("slice expects exactly three arguments"));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let start = match self.eval_expr_flow(&args[1])? {
            Flow::Value(Value::Int(value)) => value,
            Flow::Value(other) => {
                return Err(runtime_error(format!(
                    "slice start expected i64, found {}",
                    other.type_name()
                )));
            }
            flow => return Ok(flow),
        };
        let end = match self.eval_expr_flow(&args[2])? {
            Flow::Value(Value::Int(value)) => value,
            Flow::Value(other) => {
                return Err(runtime_error(format!(
                    "slice end expected i64, found {}",
                    other.type_name()
                )));
            }
            flow => return Ok(flow),
        };

        match collection {
            Value::Array(values) => {
                let (start, end) = checked_slice_bounds(start, end, values.len())?;
                Ok(Flow::Value(Value::Array(values[start..end].to_vec())))
            }
            Value::Str(value) => {
                let chars = value.chars().collect::<Vec<_>>();
                let (start, end) = checked_slice_bounds(start, end, chars.len())?;
                Ok(Flow::Value(Value::Str(chars[start..end].iter().collect())))
            }
            other => Err(runtime_error(format!(
                "slice expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_contains(&mut self, args: &[Expr]) -> FyrResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error("contains expects exactly two arguments"));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let needle = match self.eval_expr_flow(&args[1])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match (collection, needle) {
            (Value::Array(values), needle) => {
                for value in &values {
                    if values_equal(value, &needle)? {
                        return Ok(Flow::Value(Value::Bool(true)));
                    }
                }
                Ok(Flow::Value(Value::Bool(false)))
            }
            (Value::Str(value), Value::Str(needle)) => {
                Ok(Flow::Value(Value::Bool(value.contains(&needle))))
            }
            (Value::Str(_), other) => Err(runtime_error(format!(
                "contains(str, value) expected str, found {}",
                other.type_name()
            ))),
            (other, _) => Err(runtime_error(format!(
                "contains expects an array or str, found {}",
                other.type_name()
            ))),
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
        for (index, (arg, param)) in args.iter().zip(function.params.iter()).enumerate() {
            match self.eval_expr_flow(arg)? {
                Flow::Value(value) => {
                    if !value_matches_type(&value, &param.ty) {
                        return Err(runtime_error(format!(
                            "argument {} for {callee} expected {}, found {}",
                            index + 1,
                            format_type_name(&param.ty),
                            format_value_type(&value)
                        )));
                    }
                    values.push(value);
                }
                flow => return Ok(flow),
            }
        }

        self.push_scope();
        for (param, value) in function.params.iter().zip(values) {
            self.define(&param.name, value, param.ty.clone(), false)?;
        }
        let result = self.eval_block(&function.body);
        self.pop_scope();
        match result? {
            Flow::Return(value) | Flow::Value(value) => {
                if !value_matches_type(&value, &function.return_type) {
                    return Err(runtime_error(format!(
                        "return expected {}, found {}",
                        format_type_name(&function.return_type),
                        format_value_type(&value)
                    )));
                }
                Ok(Flow::Value(value))
            }
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
            let ty = infer_value_type(&value);
            self.define(name, value, ty, false)?;
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

    fn define_binding(
        &mut self,
        name: &str,
        annotation: &TypeName,
        value: Value,
        mutable: bool,
    ) -> FyrResult<()> {
        let ty = if *annotation == TypeName::Infer {
            infer_value_type(&value)
        } else {
            if !value_matches_type(&value, annotation) {
                return Err(runtime_error(format!(
                    "binding '{name}' expected {}, found {}",
                    format_type_name(annotation),
                    format_value_type(&value)
                )));
            }
            annotation.clone()
        };

        self.define(name, value, ty, mutable)
    }

    fn define(&mut self, name: &str, value: Value, ty: TypeName, mutable: bool) -> FyrResult<()> {
        if self.structs.contains_key(name) {
            return Err(runtime_error(format!("binding '{name}' already exists")));
        }

        let scope = self
            .scopes
            .last_mut()
            .expect("evaluator always has a scope");
        if scope.contains_key(name) {
            return Err(runtime_error(format!("binding '{name}' already exists")));
        }

        scope.insert(name.to_owned(), Binding { value, ty, mutable });
        Ok(())
    }

    fn current_scope(&mut self) -> &mut HashMap<String, Binding> {
        self.scopes
            .last_mut()
            .expect("evaluator always has a scope")
    }

    fn assign(&mut self, name: &str, value: Value) -> FyrResult<()> {
        for scope in self.scopes.iter_mut().rev() {
            if let Some(binding) = scope.get_mut(name) {
                if !binding.mutable {
                    return Err(runtime_error(format!(
                        "cannot assign to immutable binding '{name}'"
                    )));
                }

                if !value_matches_type(&value, &binding.ty) {
                    return Err(runtime_error(format!(
                        "assignment to '{name}' expected {}, found {}",
                        format_type_name(&binding.ty),
                        format_value_type(&value)
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

fn reject_inferred_signature(
    name: &str,
    params: &[Param],
    return_type: &TypeName,
) -> FyrResult<()> {
    for param in params {
        if param.ty == TypeName::Infer {
            return Err(runtime_error(format!(
                "function '{name}' parameter '{}' needs an explicit type",
                param.name
            )));
        }
    }

    if *return_type == TypeName::Infer {
        return Err(runtime_error(format!(
            "function '{name}' needs an explicit return type"
        )));
    }

    Ok(())
}

fn reject_duplicate_members(
    owner_kind: &str,
    owner_name: &str,
    member_kind: &str,
    members: &[Param],
) -> FyrResult<()> {
    let mut seen = HashMap::new();

    for member in members {
        if seen.insert(member.name.clone(), ()).is_some() {
            return Err(runtime_error(format!(
                "{owner_kind} '{owner_name}' has duplicate {member_kind} '{}'",
                member.name
            )));
        }
    }

    Ok(())
}

fn infer_value_type(value: &Value) -> TypeName {
    match value {
        Value::Int(_) => TypeName::I64,
        Value::Bool(_) => TypeName::Bool,
        Value::Str(_) => TypeName::Str,
        Value::Unit => TypeName::Unit,
        Value::Struct { name, .. } => TypeName::Struct(name.clone()),
        Value::Array(values) => TypeName::Array(Box::new(infer_array_element_type(values))),
        Value::Function(_) => TypeName::Infer,
    }
}

fn infer_array_element_type(values: &[Value]) -> TypeName {
    let Some(first) = values.first() else {
        return TypeName::Infer;
    };

    let first_type = infer_value_type(first);
    if values
        .iter()
        .skip(1)
        .all(|value| value_matches_type(value, &first_type))
    {
        first_type
    } else {
        TypeName::Infer
    }
}

fn value_matches_type(value: &Value, ty: &TypeName) -> bool {
    match ty {
        TypeName::Infer => true,
        TypeName::I64 => matches!(value, Value::Int(_)),
        TypeName::Bool => matches!(value, Value::Bool(_)),
        TypeName::Str => matches!(value, Value::Str(_)),
        TypeName::Unit => matches!(value, Value::Unit),
        TypeName::Struct(expected) => {
            matches!(value, Value::Struct { name, .. } if name == expected)
        }
        TypeName::Array(element) => match value {
            Value::Array(values) => values
                .iter()
                .all(|value| value_matches_type(value, element)),
            _ => false,
        },
    }
}

fn format_type_name(ty: &TypeName) -> String {
    match ty {
        TypeName::Infer => "infer".to_owned(),
        TypeName::I64 => "i64".to_owned(),
        TypeName::Bool => "bool".to_owned(),
        TypeName::Str => "str".to_owned(),
        TypeName::Unit => "unit".to_owned(),
        TypeName::Struct(name) => name.clone(),
        TypeName::Array(element) => format!("[{}]", format_type_name(element)),
    }
}

fn format_value_type(value: &Value) -> String {
    match value {
        Value::Array(values) => format_array_type(values),
        Value::Struct { name, .. } => name.clone(),
        _ => value.type_name().to_owned(),
    }
}

fn format_array_type(values: &[Value]) -> String {
    let Some(first) = values.first() else {
        return "array".to_owned();
    };

    let first_type = format_value_type(first);
    if values
        .iter()
        .skip(1)
        .all(|value| format_value_type(value) == first_type)
    {
        format!("[{first_type}]")
    } else {
        "array".to_owned()
    }
}

fn checked_int(operation: &str, value: Option<i64>) -> FyrResult<Value> {
    value
        .map(Value::Int)
        .ok_or_else(|| runtime_error(format!("integer overflow in {operation}")))
}

fn checked_slice_bounds(start: i64, end: i64, len: usize) -> FyrResult<(usize, usize)> {
    if start < 0 {
        return Err(runtime_error(format!(
            "slice start must be >= 0, found {start}"
        )));
    }
    if end < 0 {
        return Err(runtime_error(format!(
            "slice end must be >= 0, found {end}"
        )));
    }
    if start > end {
        return Err(runtime_error(format!(
            "slice start {start} must be <= end {end}"
        )));
    }

    let start = usize::try_from(start).map_err(|_| {
        runtime_error(format!(
            "slice start {start} out of bounds for length {len}"
        ))
    })?;
    let end = usize::try_from(end)
        .map_err(|_| runtime_error(format!("slice end {end} out of bounds for length {len}")))?;

    if start > len {
        return Err(runtime_error(format!(
            "slice start {start} out of bounds for length {len}"
        )));
    }
    if end > len {
        return Err(runtime_error(format!(
            "slice end {end} out of bounds for length {len}"
        )));
    }

    Ok((start, end))
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
    fn rejects_duplicate_runtime_bindings() {
        let error =
            run("let answer = 41\nlet answer = 42\n").expect_err("duplicate binding should fail");

        assert!(error.message.contains("binding 'answer' already exists"));
    }

    #[test]
    fn rejects_runtime_binding_annotation_mismatch() {
        let primitive = run("let answer: bool = 42\n").expect_err("binding annotation should fail");
        assert!(
            primitive
                .message
                .contains("binding 'answer' expected bool, found i64")
        );

        let array = run("let values: [i64] = [true]\n").expect_err("array annotation should fail");
        assert!(
            array
                .message
                .contains("binding 'values' expected [i64], found [bool]")
        );

        let nominal = run(r#"
struct Point:
    x: i64

struct Size:
    x: i64

let p: Point = Size { x: 3 }
"#)
        .expect_err("nominal annotation should fail");
        assert!(
            nominal
                .message
                .contains("binding 'p' expected Point, found Size")
        );
    }

    #[test]
    fn rejects_duplicate_runtime_functions() {
        let error = run(r#"
fn answer() -> i64:
    return 41

fn answer() -> i64:
    return 42
"#)
        .expect_err("duplicate function should fail");

        assert!(error.message.contains("binding 'answer' already exists"));
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
    fn rejects_integer_addition_overflow() {
        let error = run("9223372036854775807 + 1\n").expect_err("addition overflow should fail");

        assert!(error.message.contains("integer overflow in addition"));
    }

    #[test]
    fn rejects_integer_subtraction_overflow() {
        let error = run("let min = 0 - 9223372036854775807 - 1\nmin - 1\n")
            .expect_err("subtraction overflow should fail");

        assert!(error.message.contains("integer overflow in subtraction"));
    }

    #[test]
    fn rejects_integer_multiplication_overflow() {
        let error =
            run("9223372036854775807 * 2\n").expect_err("multiplication overflow should fail");

        assert!(error.message.contains("integer overflow in multiplication"));
    }

    #[test]
    fn rejects_integer_negation_overflow() {
        let error = run("let min = 0 - 9223372036854775807 - 1\n-min\n")
            .expect_err("negation overflow should fail");

        assert!(error.message.contains("integer overflow in negation"));
    }

    #[test]
    fn rejects_integer_division_overflow() {
        let error = run("let min = 0 - 9223372036854775807 - 1\nmin / -1\n")
            .expect_err("division overflow should fail");

        assert!(error.message.contains("integer overflow in division"));
    }

    #[test]
    fn rejects_integer_remainder_overflow() {
        let error = run("let min = 0 - 9223372036854775807 - 1\nmin % -1\n")
            .expect_err("remainder overflow should fail");

        assert!(error.message.contains("integer overflow in remainder"));
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
    fn rejects_runtime_argument_type_mismatch() {
        let error = run(r#"
fn add(a: i64, b: i64) -> i64:
    return a + b

add(1, true)
"#)
        .expect_err("argument type mismatch should fail at runtime");

        assert!(
            error
                .message
                .contains("argument 2 for add expected i64, found bool")
        );
    }

    #[test]
    fn rejects_runtime_return_type_mismatch() {
        let error = run(r#"
fn bad() -> i64:
    return true

bad()
"#)
        .expect_err("return type mismatch should fail at runtime");

        assert!(error.message.contains("return expected i64, found bool"));
    }

    #[test]
    fn rejects_untyped_runtime_function_signatures() {
        let param = run(r#"
fn add(a, b: i64) -> i64:
    return b
"#)
        .expect_err("untyped parameter should fail at runtime");
        assert!(
            param
                .message
                .contains("function 'add' parameter 'a' needs an explicit type")
        );

        let return_type = run(r#"
fn add(a: i64):
    return a
"#)
        .expect_err("untyped return should fail at runtime");
        assert!(
            return_type
                .message
                .contains("function 'add' needs an explicit return type")
        );
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
    fn runs_local_functions_after_declaration() {
        let result = run(r#"
fn outer(value: i64) -> i64:
    fn double(input: i64) -> i64:
        return input * 2

    return double(value)

outer(21)
"#)
        .expect("local function should run after declaration");

        assert_eq!(result.last_value, Value::Int(42));
    }

    #[test]
    fn runs_recursive_local_functions() {
        let result = run(r#"
fn outer(value: i64) -> i64:
    fn countdown(n: i64) -> i64:
        if n == 0:
            return value
        else:
            return countdown(n - 1)

    return countdown(3)

outer(42)
"#)
        .expect("recursive local function should run");

        assert_eq!(result.last_value, Value::Int(42));
    }

    #[test]
    fn rejects_runtime_parameter_redeclaration() {
        let error = run(r#"
fn echo(value: i64) -> i64:
    let value = 42
    return value

echo(1)
"#)
        .expect_err("parameter redeclaration should fail");

        assert!(error.message.contains("binding 'value' already exists"));
    }

    #[test]
    fn rejects_runtime_for_variable_redeclaration() {
        let error = run(r#"
for value in [1]:
    let value = 2
"#)
        .expect_err("for variable redeclaration should fail");

        assert!(error.message.contains("binding 'value' already exists"));
    }

    #[test]
    fn rejects_duplicate_runtime_function_parameters() {
        let error = run(r#"
fn choose(value: i64, value: i64) -> i64:
    return value

choose(1, 2)
"#)
        .expect_err("duplicate function parameter should fail");

        assert!(
            error
                .message
                .contains("function 'choose' has duplicate parameter 'value'")
        );
    }

    #[test]
    fn rejects_runtime_assignment_type_changes() {
        let primitive = run(r#"
var value = 1
value = "one"
"#)
        .expect_err("primitive assignment type change should fail");
        assert!(
            primitive
                .message
                .contains("assignment to 'value' expected i64, found str")
        );

        let array = run(r#"
var values = [1]
values = [true]
"#)
        .expect_err("array assignment type change should fail");
        assert!(
            array
                .message
                .contains("assignment to 'values' expected [i64], found [bool]")
        );

        let nominal = run(r#"
struct Point:
    x: i64

struct Size:
    x: i64

var p = Point { x: 3 }
p = Size { x: 3 }
"#)
        .expect_err("nominal assignment type change should fail");
        assert!(
            nominal
                .message
                .contains("assignment to 'p' expected Point, found Size")
        );
    }

    #[test]
    fn assigns_to_runtime_typed_empty_arrays() {
        let result = run(r#"
var values: [i64] = []
values = [3, 5, 8]
len(values)
"#)
        .expect("typed empty array assignment should run");

        assert_eq!(result.last_value, Value::Int(3));
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
    fn supports_forward_struct_literals() {
        let result = run(r#"
let p = Point { x: 3, y: 4 }

struct Point:
    x: i64
    y: i64

p.x + p.y
"#)
        .expect("forward struct literal should run");

        assert_eq!(result.last_value, Value::Int(7));
    }

    #[test]
    fn rejects_unknown_runtime_structs() {
        let error =
            run("Point { x: 3 }\n").expect_err("unknown struct literal should fail at runtime");

        assert!(error.message.contains("unknown struct 'Point'"));
    }

    #[test]
    fn rejects_duplicate_runtime_struct_declarations() {
        let error = run(r#"
struct Point:
    x: i64

struct Point:
    x: i64
"#)
        .expect_err("duplicate struct declaration should fail at runtime");

        assert!(error.message.contains("struct 'Point' already exists"));
    }

    #[test]
    fn rejects_duplicate_runtime_struct_fields() {
        let error = run(r#"
struct Point:
    x: i64
    x: bool
"#)
        .expect_err("duplicate struct field should fail at runtime");

        assert!(
            error
                .message
                .contains("struct 'Point' has duplicate field 'x'")
        );
    }

    #[test]
    fn rejects_runtime_struct_literal_field_errors() {
        let duplicate = run(r#"
struct Point:
    x: i64
    y: i64

Point { x: 3, x: 4, y: 5 }
"#)
        .expect_err("duplicate struct literal field should fail at runtime");
        assert!(
            duplicate
                .message
                .contains("field 'x' initialized more than once")
        );

        let unknown = run(r#"
struct Point:
    x: i64

Point { x: 3, y: 4 }
"#)
        .expect_err("unknown struct literal field should fail at runtime");
        assert!(unknown.message.contains("struct 'Point' has no field 'y'"));

        let missing = run(r#"
struct Point:
    x: i64
    y: i64

Point { x: 3 }
"#)
        .expect_err("missing struct literal field should fail at runtime");
        assert!(missing.message.contains("struct 'Point' missing field 'y'"));

        let primitive_mismatch = run(r#"
struct Point:
    x: i64

Point { x: true }
"#)
        .expect_err("struct primitive field mismatch should fail at runtime");
        assert!(
            primitive_mismatch
                .message
                .contains("field 'x' expected i64, found bool")
        );

        let array_mismatch = run(r#"
struct Row:
    values: [i64]

Row { values: [true] }
"#)
        .expect_err("struct array field mismatch should fail at runtime");
        assert!(
            array_mismatch
                .message
                .contains("field 'values' expected [i64], found [bool]")
        );

        let struct_mismatch = run(r#"
struct Point:
    x: i64

struct Line:
    start: Point

struct Size:
    x: i64

Line { start: Size { x: 3 } }
"#)
        .expect_err("struct nominal field mismatch should fail at runtime");
        assert!(
            struct_mismatch
                .message
                .contains("field 'start' expected Point, found Size")
        );
    }

    #[test]
    fn rejects_runtime_names_that_collide_with_structs() {
        let binding = run(r#"
struct Point:
    x: i64

let Point = 42
"#)
        .expect_err("binding and struct collision should fail at runtime");
        assert!(binding.message.contains("binding 'Point' already exists"));

        let function = run(r#"
struct Point:
    x: i64

fn Point() -> i64:
    return 42
"#)
        .expect_err("function and struct collision should fail at runtime");
        assert!(function.message.contains("binding 'Point' already exists"));
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
    fn supports_word_boolean_operators() {
        let result = run("not false and false or true\n").expect("word booleans should run");

        assert_eq!(result.last_value, Value::Bool(true));
    }

    #[test]
    fn supports_elif_branches() {
        let result = run(r#"
fn label(value: i64) -> str:
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    elif value == 1:
        return "one"
    else:
        return "many"

label(1)
"#)
        .expect("elif branches should run");

        assert_eq!(result.last_value, Value::Str("one".to_owned()));
    }

    #[test]
    fn supports_elif_if_expressions() {
        let result = run(r#"
let value = 0
let label = if value < 0:
    "negative"
elif value == 0:
    "zero"
else:
    "positive"

label
"#)
        .expect("elif expression should run");

        assert_eq!(result.last_value, Value::Str("zero".to_owned()));
    }

    #[test]
    fn rejects_failed_assertions() {
        let error =
            run("assert(false, \"expected failure\")\n").expect_err("assertion should fail");

        assert!(error.message.contains("assertion failed: expected failure"));
    }

    #[test]
    fn supports_contains() {
        let result = run(r#"
struct Point:
    x: i64
    y: i64

let points = [Point { x: 3, y: 4 }]

assert(contains([1, 2, 3], 2))
assert(not contains([1, 2, 3], 4))
assert(contains("secure Fyr", "Fyr"))
assert(contains(points, Point { x: 3, y: 4 }))
"#)
        .expect("contains should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_slice_for_arrays_and_strings() {
        let result = run(r#"
assert(slice([3, 5, 8, 13, 21], 1, 4) == [5, 8, 13])
assert(slice([3, 5, 8], 2, 2) == [])
assert(slice("secure Fyr", 0, 6) == "secure")
assert(slice("Fyr", 1, 3) == "yr")
"#)
        .expect("slice should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_is_empty_for_arrays_and_strings() {
        let result = run(r#"
assert(is_empty([]))
assert(not is_empty([1]))
assert(is_empty(""))
assert(not is_empty("Fyr"))
"#)
        .expect("is_empty should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_get_with_fallbacks() {
        let result = run(r#"
assert(get([3, 5, 8], 1, -1) == 5)
assert(get([3, 5, 8], 9, -1) == -1)
assert(get([3, 5, 8], -1, -1) == -1)
assert(get([], 0, 42) == 42)
assert(get("Fyr", 1, "?") == "y")
assert(get("Fyr", 9, "?") == "?")
"#)
        .expect("get should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_find_for_arrays_and_strings() {
        let result = run(r#"
struct Point:
    x: i64
    y: i64

let points = [Point { x: 3, y: 4 }, Point { x: 5, y: 12 }]

assert(find([3, 5, 8], 5) == 1)
assert(find([3, 5, 8], 21) == -1)
assert(find([], 21) == -1)
assert(find(points, Point { x: 5, y: 12 }) == 1)
assert(find("secure Fyr", "Fyr") == 7)
assert(find("secure Fyr", "missing") == -1)
"#)
        .expect("find should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_count_for_arrays_and_strings() {
        let result = run(r#"
struct Point:
    x: i64
    y: i64

let points = [Point { x: 3, y: 4 }, Point { x: 3, y: 4 }, Point { x: 5, y: 12 }]

assert(count([3, 5, 3, 8, 3], 3) == 3)
assert(count([3, 5, 8], 21) == 0)
assert(count([], 21) == 0)
assert(count(points, Point { x: 3, y: 4 }) == 2)
assert(count("secure Fyr secure", "secure") == 2)
assert(count("aaaa", "aa") == 2)
assert(count("secure Fyr", "missing") == 0)
assert(count("Fyr", "") == 0)
"#)
        .expect("count should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn rejects_find_runtime_type_errors() {
        let collection = run("find(42, 1)\n").expect_err("find collection should fail");
        assert!(collection.message.contains("find expects an array or str"));

        let needle = run("find(\"Fyr\", 1)\n").expect_err("find string needle should fail");
        assert!(needle.message.contains("find(str, value) expected str"));
    }

    #[test]
    fn rejects_count_runtime_type_errors() {
        let collection = run("count(42, 1)\n").expect_err("count collection should fail");
        assert!(collection.message.contains("count expects an array or str"));

        let needle = run("count(\"Fyr\", 1)\n").expect_err("count string needle should fail");
        assert!(needle.message.contains("count(str, value) expected str"));
    }

    #[test]
    fn rejects_get_runtime_type_errors() {
        let collection = run("get(42, 0, 1)\n").expect_err("get collection should fail");
        assert!(collection.message.contains("get expects an array or str"));

        let index = run("get([1, 2, 3], true, 0)\n").expect_err("get index should fail");
        assert!(index.message.contains("get index expected i64"));
    }

    #[test]
    fn rejects_is_empty_runtime_type_errors() {
        let error = run("is_empty(42)\n").expect_err("is_empty collection should fail");

        assert!(error.message.contains("is_empty expects an array or str"));
    }

    #[test]
    fn rejects_slice_bounds_errors() {
        let negative_start =
            run("slice([1, 2, 3], -1, 2)\n").expect_err("negative start should fail");
        assert!(negative_start.message.contains("slice start must be >= 0"));

        let reversed = run("slice([1, 2, 3], 2, 1)\n").expect_err("reversed slice should fail");
        assert!(reversed.message.contains("must be <= end"));

        let too_far = run("slice(\"Fyr\", 0, 4)\n").expect_err("oversized end should fail");
        assert!(too_far.message.contains("slice end 4 out of bounds"));
    }

    #[test]
    fn rejects_slice_runtime_type_errors() {
        let start = run("slice([1, 2, 3], true, 2)\n").expect_err("start type should fail");
        assert!(start.message.contains("slice start expected i64"));

        let collection = run("slice(42, 0, 1)\n").expect_err("collection type should fail");
        assert!(collection.message.contains("slice expects an array or str"));
    }

    #[test]
    fn rejects_contains_type_errors_at_runtime() {
        let error = run("contains(\"fyr\", 1)\n").expect_err("contains should fail");

        assert!(error.message.contains("contains(str, value) expected str"));
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
    fn appends_to_arrays() {
        let result = run(r#"
let first = append(append([], 3), 5)
let second = append(first, 8)
let third = append(second, 13)
append(third, 21)
"#)
        .expect("append should run");

        assert_eq!(
            result.last_value,
            Value::Array(vec![
                Value::Int(3),
                Value::Int(5),
                Value::Int(8),
                Value::Int(13),
                Value::Int(21)
            ])
        );
    }

    #[test]
    fn rejects_append_runtime_type_errors() {
        let error = run("append(42, 1)\n").expect_err("append collection should fail");

        assert!(error.message.contains("append expects an array"));
    }

    #[test]
    fn rejects_out_of_bounds_array_index() {
        let error = run("[1, 2][2]\n").expect_err("out-of-bounds index should fail");

        assert!(error.message.contains("out of bounds"));
    }
}

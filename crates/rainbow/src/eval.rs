use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use crate::ast::{
    BinaryOp, EnumVariant, Expr, IfLetPattern, MatchArm, MatchPattern, Param, Program, Statement,
    TypeName, UnaryOp,
};
use crate::diagnostic::{RainbowError, RainbowResult};
use crate::span::Span;

const EXACT_F64_INTEGER_LIMIT: i64 = 9_007_199_254_740_992;
const EXACT_F64_INTEGER_LIMIT_F64: f64 = 9_007_199_254_740_992.0;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Nil,
    Array(Vec<Value>),
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    Enum {
        name: String,
        variant: String,
        payload: Option<Box<Value>>,
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
            Value::Float(value) => write!(f, "{}", format_float(*value)),
            Value::Bool(value) => write!(f, "{value}"),
            Value::Str(value) => write!(f, "{value}"),
            Value::Nil => write!(f, "nil"),
            Value::Array(values) => {
                let values = values
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(f, "[{values}]")
            }
            Value::Struct { name, .. } => write!(f, "<{name}>"),
            Value::Enum {
                name,
                variant,
                payload,
            } => {
                if let Some(payload) = payload {
                    write!(f, "{name}.{variant}({payload})")
                } else {
                    write!(f, "{name}.{variant}")
                }
            }
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
    enums: HashMap<String, Vec<EnumVariant>>,
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

#[derive(Debug, Clone, PartialEq)]
enum IfLetMatch {
    NoMatch,
    Match {
        binding: Option<(String, Value, TypeName)>,
    },
}

#[derive(Debug, Clone, Copy)]
enum Edge {
    First,
    Last,
}

impl Edge {
    fn name(self) -> &'static str {
        match self {
            Edge::First => "first",
            Edge::Last => "last",
        }
    }

    fn default_context(self) -> &'static str {
        match self {
            Edge::First => "first default",
            Edge::Last => "last default",
        }
    }

    fn array_value(self, values: &[Value]) -> Option<Value> {
        match self {
            Edge::First => values.first().cloned(),
            Edge::Last => values.last().cloned(),
        }
    }

    fn string_value(self, value: &str) -> Option<char> {
        match self {
            Edge::First => value.chars().next(),
            Edge::Last => value.chars().last(),
        }
    }
}

impl Evaluator {
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
            structs: HashMap::new(),
            enums: HashMap::new(),
            outputs: Vec::new(),
        }
    }

    pub fn run(mut self, program: &Program) -> RainbowResult<RunResult> {
        self.predefine_enums(&program.statements)?;
        self.predefine_structs(&program.statements)?;
        self.predefine_functions(&program.statements)?;

        let mut last_value = Value::Unit;

        for statement in &program.statements {
            if matches!(
                statement,
                Statement::Struct { .. }
                    | Statement::Enum { .. }
                    | Statement::Fn { .. }
                    | Statement::Import { .. }
            ) {
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

    pub fn eval_statement(&mut self, statement: &Statement) -> RainbowResult<Value> {
        match self.eval_statement_flow(statement)? {
            Flow::Value(value) => Ok(value),
            Flow::Return(_) => Err(runtime_error("return outside function")),
            Flow::Break => Err(runtime_error("break outside loop")),
            Flow::Continue => Err(runtime_error("continue outside loop")),
        }
    }

    pub fn predefine_declarations(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        self.predefine_enums(statements)?;
        self.predefine_structs(statements)?;
        self.predefine_functions(statements)?;
        Ok(())
    }

    fn eval_statement_flow(&mut self, statement: &Statement) -> RainbowResult<Flow> {
        let span = statement.span();
        let source_path = statement.source_path();
        let result = match statement {
            Statement::Struct { name, fields, .. } => {
                self.define_struct(name, fields)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Enum { name, variants, .. } => {
                self.define_enum(name, variants)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Import { .. } => Ok(Flow::Value(Value::Unit)),
            Statement::Let {
                name, ty, value, ..
            } => {
                let value = self.eval_value(value)?;
                self.define_binding(name, ty, value, false)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Var {
                name, ty, value, ..
            } => {
                let value = self.eval_value(value)?;
                self.define_binding(name, ty, value, true)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Assign { name, value, .. } => {
                let value = self.eval_value(value)?;
                self.assign(name, value)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::Fn {
                name,
                params,
                return_type,
                body,
                ..
            } => {
                self.define_function(name, params, return_type, body)?;
                Ok(Flow::Value(Value::Unit))
            }
            Statement::While {
                condition, body, ..
            } => self.eval_while(condition, body),
            Statement::For {
                name,
                iterable,
                body,
                ..
            } => self.eval_for(name, iterable, body),
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => self.eval_if(condition, then_branch, else_branch),
            Statement::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
                ..
            } => self.eval_if_let(pattern, value, then_branch, else_branch),
            Statement::Return { value, .. } => {
                let value = match value {
                    Some(value) => self.eval_value(value)?,
                    None => Value::Unit,
                };
                Ok(Flow::Return(value))
            }
            Statement::Break { .. } => Ok(Flow::Break),
            Statement::Continue { .. } => Ok(Flow::Continue),
            Statement::Expr { expr, .. } => self.eval_expr_flow(expr),
        };

        result.map_err(|error| error.with_fallback_location(span, source_path))
    }

    fn predefine_structs(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            if let Statement::Struct { name, fields, .. } = statement {
                self.define_struct(name, fields)?;
            }
        }

        Ok(())
    }

    fn predefine_enums(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            if let Statement::Enum { name, variants, .. } = statement {
                self.define_enum(name, variants)?;
            }
        }

        Ok(())
    }

    fn predefine_functions(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            if let Statement::Fn {
                name,
                params,
                return_type,
                body,
                ..
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
    ) -> RainbowResult<()> {
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

    fn define_struct(&mut self, name: &str, fields: &[Param]) -> RainbowResult<()> {
        if self.structs.contains_key(name)
            || self.enums.contains_key(name)
            || self.current_scope().contains_key(name)
        {
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

    fn define_enum(&mut self, name: &str, variants: &[EnumVariant]) -> RainbowResult<()> {
        if self.enums.contains_key(name)
            || self.structs.contains_key(name)
            || self.current_scope().contains_key(name)
        {
            return Err(runtime_error(format!("enum '{name}' already exists")));
        }

        let mut seen = HashMap::new();
        for variant in variants {
            if seen.insert(variant.name.clone(), ()).is_some() {
                return Err(runtime_error(format!(
                    "enum '{name}' has duplicate variant '{}'",
                    variant.name
                )));
            }
        }

        self.enums.insert(name.to_owned(), variants.to_vec());
        Ok(())
    }

    fn eval_value(&mut self, expr: &Expr) -> RainbowResult<Value> {
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

    fn eval_expr_flow(&mut self, expr: &Expr) -> RainbowResult<Flow> {
        match expr {
            Expr::Int(value) => Ok(Flow::Value(Value::Int(*value))),
            Expr::Float(value) => Ok(Flow::Value(Value::Float(*value))),
            Expr::Bool(value) => Ok(Flow::Value(Value::Bool(*value))),
            Expr::Str(value) => Ok(Flow::Value(Value::Str(value.clone()))),
            Expr::Nil => Ok(Flow::Value(Value::Nil)),
            Expr::Variable(name) => self.lookup(name).cloned().map(Flow::Value).ok_or_else(|| {
                RainbowError::new(format!("unknown binding '{name}'"), Span::new(0, 0))
            }),
            Expr::Unary { op, expr } => {
                let value = match self.eval_expr_flow(expr)? {
                    Flow::Value(value) => value,
                    flow => return Ok(flow),
                };
                Ok(Flow::Value(self.eval_unary(*op, value)?))
            }
            Expr::Binary { left, op, right } => self.eval_binary(left, *op, right),
            Expr::Call { callee, args } => self.eval_call(callee, args),
            Expr::Flow {
                value,
                callee,
                args,
            } => self.eval_flow(value, callee, args),
            Expr::StructInit { name, fields } => self.eval_struct_init(name, fields),
            Expr::EnumInit {
                enum_name,
                variant,
                value,
            } => self.eval_enum_init(enum_name, variant, value.as_deref()),
            Expr::Field { object, field } => self.eval_field(object, field),
            Expr::Array(elements) => self.eval_array(elements),
            Expr::Index { collection, index } => self.eval_index(collection, index),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.eval_if(condition, then_branch, else_branch),
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => self.eval_if_let(pattern, value, then_branch, else_branch),
            Expr::Match { value, arms } => self.eval_match(value, arms),
        }
    }

    fn eval_array(&mut self, elements: &[Expr]) -> RainbowResult<Flow> {
        let mut values = Vec::with_capacity(elements.len());

        for element in elements {
            match self.eval_expr_flow(element)? {
                Flow::Value(value) => values.push(value),
                flow => return Ok(flow),
            }
        }

        ensure_homogeneous_array(&values, "array element")?;
        Ok(Flow::Value(Value::Array(values)))
    }

    fn eval_index(&mut self, collection: &Expr, index: &Expr) -> RainbowResult<Flow> {
        let collection = match self.eval_expr_flow(collection)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };
        let index = match self.eval_expr_flow(index)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match collection {
            Value::Array(values) => {
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
            Value::Str(value) => {
                let index = match index {
                    Value::Int(index) => index,
                    other => {
                        return Err(runtime_error(format!(
                            "string index expected i64, found {}",
                            other.type_name()
                        )));
                    }
                };

                if index < 0 {
                    return Err(runtime_error(format!("string index {index} out of bounds")));
                }

                value
                    .chars()
                    .nth(index as usize)
                    .map(|ch| Flow::Value(Value::Str(ch.to_string())))
                    .ok_or_else(|| runtime_error(format!("string index {index} out of bounds")))
            }
            other => Err(runtime_error(format!(
                "indexing expected an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_struct_init(&mut self, name: &str, fields: &[(String, Expr)]) -> RainbowResult<Flow> {
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

    fn eval_field(&mut self, object: &Expr, field: &str) -> RainbowResult<Flow> {
        if let Expr::Variable(enum_name) = object
            && let Some(variants) = self.enums.get(enum_name)
        {
            if let Some(variant) = variants.iter().find(|variant| variant.name == field) {
                if variant.payload.is_some() {
                    return Err(runtime_error(format!(
                        "enum variant '{enum_name}.{field}' needs a payload"
                    )));
                }
                return Ok(Flow::Value(Value::Enum {
                    name: enum_name.clone(),
                    variant: field.to_owned(),
                    payload: None,
                }));
            }

            return Err(runtime_error(format!(
                "enum '{enum_name}' has no variant '{field}'"
            )));
        }

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

    fn eval_enum_init(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        value: Option<&Expr>,
    ) -> RainbowResult<Flow> {
        let variant = self
            .enums
            .get(enum_name)
            .and_then(|variants| variants.iter().find(|variant| variant.name == variant_name))
            .cloned()
            .ok_or_else(|| {
                if self.enums.contains_key(enum_name) {
                    runtime_error(format!(
                        "enum '{enum_name}' has no variant '{variant_name}'"
                    ))
                } else {
                    runtime_error(format!("unknown enum '{enum_name}'"))
                }
            })?;

        let payload = match (&variant.payload, value) {
            (Some(expected), Some(value)) => {
                let value = match self.eval_expr_flow(value)? {
                    Flow::Value(value) => value,
                    flow => return Ok(flow),
                };
                if !value_matches_type(&value, expected) {
                    return Err(runtime_error(format!(
                        "enum variant '{enum_name}.{variant_name}' expected {}, found {}",
                        format_type_name(expected),
                        format_value_type(&value)
                    )));
                }
                Some(Box::new(value))
            }
            (Some(expected), None) => {
                return Err(runtime_error(format!(
                    "enum variant '{enum_name}.{variant_name}' expected payload {}",
                    format_type_name(expected)
                )));
            }
            (None, Some(_)) => {
                return Err(runtime_error(format!(
                    "enum variant '{enum_name}.{variant_name}' does not take a payload"
                )));
            }
            (None, None) => {
                return Err(runtime_error(format!(
                    "enum variant '{enum_name}.{variant_name}' is a value and should not be called"
                )));
            }
        };

        Ok(Flow::Value(Value::Enum {
            name: enum_name.to_owned(),
            variant: variant_name.to_owned(),
            payload,
        }))
    }

    fn eval_unary(&self, op: UnaryOp, value: Value) -> RainbowResult<Value> {
        match (op, value) {
            (UnaryOp::Negate, Value::Int(value)) => checked_int("negation", value.checked_neg()),
            (UnaryOp::Negate, Value::Float(value)) => checked_float("negation", -value),
            (UnaryOp::Not, Value::Bool(value)) => Ok(Value::Bool(!value)),
            (UnaryOp::Negate, other) => type_error("i64 or f64", &other),
            (UnaryOp::Not, other) => type_error("bool", &other),
        }
    }

    fn eval_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> RainbowResult<Flow> {
        if op == BinaryOp::Coalesce {
            let left = match self.eval_expr_flow(left)? {
                Flow::Value(value) => value,
                flow => return Ok(flow),
            };
            return match left {
                Value::Nil => self.eval_expr_flow(right),
                value => Ok(Flow::Value(value)),
            };
        }

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
            (Value::Float(left), BinaryOp::Add, Value::Float(right)) => {
                checked_float("addition", left + right)?
            }
            (Value::Float(left), BinaryOp::Subtract, Value::Float(right)) => {
                checked_float("subtraction", left - right)?
            }
            (Value::Float(left), BinaryOp::Multiply, Value::Float(right)) => {
                checked_float("multiplication", left * right)?
            }
            (Value::Float(_), BinaryOp::Divide, Value::Float(0.0)) => {
                return Err(runtime_error("division by zero"));
            }
            (Value::Float(left), BinaryOp::Divide, Value::Float(right)) => {
                checked_float("division", left / right)?
            }
            (Value::Float(_), BinaryOp::Remainder, Value::Float(0.0)) => {
                return Err(runtime_error("remainder by zero"));
            }
            (Value::Float(left), BinaryOp::Remainder, Value::Float(right)) => {
                checked_float("remainder", left % right)?
            }
            (Value::Str(left), BinaryOp::Add, Value::Str(right)) => {
                Value::Str(format!("{left}{right}"))
            }
            (Value::Array(mut left), BinaryOp::Add, right @ Value::Array(_)) => {
                ensure_array_concat_matches(&left, &right)?;
                let Value::Array(right) = right else {
                    unreachable!("array concatenation already matched arrays")
                };
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
            (Value::Float(left), BinaryOp::Less, Value::Float(right)) => Value::Bool(left < right),
            (Value::Float(left), BinaryOp::LessEqual, Value::Float(right)) => {
                Value::Bool(left <= right)
            }
            (Value::Float(left), BinaryOp::Greater, Value::Float(right)) => {
                Value::Bool(left > right)
            }
            (Value::Float(left), BinaryOp::GreaterEqual, Value::Float(right)) => {
                Value::Bool(left >= right)
            }
            (left, op, right) => Err(runtime_error(format!(
                "operator '{op:?}' cannot be applied to {left:?} and {right:?}"
            )))?,
        };

        Ok(Flow::Value(value))
    }

    fn expect_bool(&mut self, expr: &Expr) -> RainbowResult<Flow> {
        match self.eval_expr_flow(expr)? {
            Flow::Value(Value::Bool(value)) => Ok(Flow::Value(Value::Bool(value))),
            Flow::Value(other) => type_error("bool", &other),
            flow => Ok(flow),
        }
    }

    fn eval_call(&mut self, callee: &str, args: &[Expr]) -> RainbowResult<Flow> {
        match callee {
            "i64" => self.eval_numeric_conversion(callee, args),
            "f64" => self.eval_numeric_conversion(callee, args),
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
            "reverse" => self.eval_reverse(args),
            "first" => self.eval_edge(args, Edge::First),
            "last" => self.eval_edge(args, Edge::Last),
            "is_empty" => self.eval_is_empty(args),
            "get" => self.eval_get(args),
            "find" => self.eval_find(args),
            "count" => self.eval_count(args),
            "trim" => self.eval_string_transform(args, "trim", |value| value.trim().to_owned()),
            "lower" => self.eval_string_transform(args, "lower", |value| value.to_lowercase()),
            "upper" => self.eval_string_transform(args, "upper", |value| value.to_uppercase()),
            "starts_with" => self.eval_string_predicate(args, "starts_with", |value, prefix| {
                value.starts_with(prefix)
            }),
            "ends_with" => self
                .eval_string_predicate(args, "ends_with", |value, suffix| value.ends_with(suffix)),
            "replace" => self.eval_replace(args),
            "split" => self.eval_split(args),
            "join" => self.eval_join(args),
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
                Ok(Flow::Value(Value::Str(format_value_type(&value))))
            }
            other => self.eval_user_call(other, args),
        }
    }

    fn eval_flow(&mut self, value: &Expr, callee: &str, args: &[Expr]) -> RainbowResult<Flow> {
        let mut flowd_args = Vec::with_capacity(args.len() + 1);
        flowd_args.push(value.clone());
        flowd_args.extend(args.iter().cloned());
        self.eval_call(callee, &flowd_args)
    }

    fn eval_numeric_conversion(&mut self, name: &str, args: &[Expr]) -> RainbowResult<Flow> {
        if args.len() != 1 {
            return Err(runtime_error(format!(
                "{name} expects exactly one argument"
            )));
        }

        let value = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match (name, value) {
            ("i64", Value::Int(value)) => Ok(Flow::Value(Value::Int(value))),
            ("i64", Value::Float(value)) => {
                if !value.is_finite() {
                    return Err(runtime_error("i64 conversion expected a finite f64 value"));
                }
                if value.fract() != 0.0 {
                    return Err(runtime_error("i64 conversion expected a whole f64 value"));
                }
                if !(-EXACT_F64_INTEGER_LIMIT_F64..=EXACT_F64_INTEGER_LIMIT_F64).contains(&value) {
                    return Err(runtime_error(
                        "i64 conversion expected a value inside the exact f64 integer range",
                    ));
                }
                Ok(Flow::Value(Value::Int(value as i64)))
            }
            ("f64", Value::Float(value)) => Ok(Flow::Value(Value::Float(value))),
            ("f64", Value::Int(value)) => {
                if !(-EXACT_F64_INTEGER_LIMIT..=EXACT_F64_INTEGER_LIMIT).contains(&value) {
                    return Err(runtime_error("f64 conversion would lose integer precision"));
                }
                Ok(Flow::Value(Value::Float(value as f64)))
            }
            ("i64" | "f64", other) => Err(runtime_error(format!(
                "{name} conversion expects i64 or f64, found {}",
                format_value_type(&other)
            ))),
            _ => unreachable!("numeric conversion called with non-numeric builtin"),
        }
    }

    fn eval_string_transform(
        &mut self,
        args: &[Expr],
        name: &str,
        transform: fn(String) -> String,
    ) -> RainbowResult<Flow> {
        if args.len() != 1 {
            return Err(runtime_error(format!(
                "{name} expects exactly one argument"
            )));
        }

        let value = self.eval_string_arg(&args[0], name)?;
        Ok(Flow::Value(Value::Str(transform(value))))
    }

    fn eval_string_predicate(
        &mut self,
        args: &[Expr],
        name: &str,
        predicate: fn(&str, &str) -> bool,
    ) -> RainbowResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error(format!(
                "{name} expects exactly two arguments"
            )));
        }

        let value = self.eval_string_arg(&args[0], name)?;
        let needle = self.eval_string_arg(&args[1], &format!("{name} value"))?;
        Ok(Flow::Value(Value::Bool(predicate(&value, &needle))))
    }

    fn eval_replace(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
        if args.len() != 3 {
            return Err(runtime_error("replace expects exactly three arguments"));
        }

        let value = self.eval_string_arg(&args[0], "replace")?;
        let needle = self.eval_string_arg(&args[1], "replace old")?;
        if needle.is_empty() {
            return Err(runtime_error("replace old value must not be empty"));
        }
        let replacement = self.eval_string_arg(&args[2], "replace new")?;

        Ok(Flow::Value(Value::Str(
            value.replace(&needle, &replacement),
        )))
    }

    fn eval_split(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error("split expects exactly two arguments"));
        }

        let value = self.eval_string_arg(&args[0], "split")?;
        let separator = self.eval_string_arg(&args[1], "split separator")?;
        if separator.is_empty() {
            return Err(runtime_error("split separator must not be empty"));
        }

        Ok(Flow::Value(Value::Array(
            value
                .split(&separator)
                .map(|part| Value::Str(part.to_owned()))
                .collect(),
        )))
    }

    fn eval_join(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error("join expects exactly two arguments"));
        }

        let parts = match self.eval_expr_flow(&args[0])? {
            Flow::Value(Value::Array(values)) => values,
            Flow::Value(other) => {
                return Err(runtime_error(format!(
                    "join expects [str], found {}",
                    format_value_type(&other)
                )));
            }
            flow => return Ok(flow),
        };
        let separator = self.eval_string_arg(&args[1], "join separator")?;
        let parts = expect_string_array(parts, "join")?;

        Ok(Flow::Value(Value::Str(parts.join(&separator))))
    }

    fn eval_string_arg(&mut self, arg: &Expr, context: &str) -> RainbowResult<String> {
        match self.eval_value(arg)? {
            Value::Str(value) => Ok(value),
            other => Err(runtime_error(format!(
                "{context} expected str, found {}",
                format_value_type(&other)
            ))),
        }
    }

    fn eval_count(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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
                ensure_array_item_matches(&values, &needle, "count")?;
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

    fn eval_find(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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
                ensure_array_item_matches(&values, &needle, "find")?;
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

    fn eval_get(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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
                None => {
                    let fallback = match self.eval_expr_flow(&args[2])? {
                        Flow::Value(value) => value,
                        flow => return Ok(flow),
                    };
                    ensure_array_fallback_matches(&values, &fallback, "get default")?;
                    Ok(Flow::Value(fallback))
                }
            },
            Value::Str(value) => match usize::try_from(index)
                .ok()
                .and_then(|index| value.chars().nth(index))
            {
                Some(ch) => Ok(Flow::Value(Value::Str(ch.to_string()))),
                None => match self.eval_expr_flow(&args[2])? {
                    Flow::Value(Value::Str(value)) => Ok(Flow::Value(Value::Str(value))),
                    Flow::Value(other) => Err(runtime_error(format!(
                        "get default expected str, found {}",
                        format_value_type(&other)
                    ))),
                    flow => Ok(flow),
                },
            },
            other => Err(runtime_error(format!(
                "get expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_is_empty(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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

    fn eval_append(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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
                ensure_array_item_matches(&values, &value, "append")?;
                values.push(value);
                Ok(Flow::Value(Value::Array(values)))
            }
            other => Err(runtime_error(format!(
                "append expects an array, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_reverse(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
        if args.len() != 1 {
            return Err(runtime_error("reverse expects exactly one argument"));
        }

        let value = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match value {
            Value::Array(mut values) => {
                ensure_homogeneous_array(&values, "array element")?;
                values.reverse();
                Ok(Flow::Value(Value::Array(values)))
            }
            Value::Str(value) => Ok(Flow::Value(Value::Str(value.chars().rev().collect()))),
            other => Err(runtime_error(format!(
                "reverse expects an array or str, found {}",
                other.type_name()
            ))),
        }
    }

    fn eval_edge(&mut self, args: &[Expr], edge: Edge) -> RainbowResult<Flow> {
        if args.len() != 2 {
            return Err(runtime_error(format!(
                "{} expects exactly two arguments",
                edge.name()
            )));
        }

        let collection = match self.eval_expr_flow(&args[0])? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        match collection {
            Value::Array(values) => match edge.array_value(&values) {
                Some(value) => Ok(Flow::Value(value)),
                None => {
                    let fallback = match self.eval_expr_flow(&args[1])? {
                        Flow::Value(value) => value,
                        flow => return Ok(flow),
                    };
                    ensure_array_fallback_matches(&values, &fallback, edge.default_context())?;
                    Ok(Flow::Value(fallback))
                }
            },
            Value::Str(value) => match edge.string_value(&value) {
                Some(value) => Ok(Flow::Value(Value::Str(value.to_string()))),
                None => match self.eval_expr_flow(&args[1])? {
                    Flow::Value(Value::Str(value)) => Ok(Flow::Value(Value::Str(value))),
                    Flow::Value(other) => Err(runtime_error(format!(
                        "{} default expected str, found {}",
                        edge.name(),
                        format_value_type(&other)
                    ))),
                    flow => Ok(flow),
                },
            },
            other => Err(runtime_error(format!(
                "{} expects an array or str, found {}",
                edge.name(),
                other.type_name()
            ))),
        }
    }

    fn eval_slice(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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

    fn eval_contains(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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
                ensure_array_item_matches(&values, &needle, "contains")?;
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

    fn eval_range(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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

    fn eval_assert(&mut self, args: &[Expr]) -> RainbowResult<Flow> {
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

    fn eval_user_call(&mut self, callee: &str, args: &[Expr]) -> RainbowResult<Flow> {
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
    ) -> RainbowResult<Flow> {
        match self.eval_expr_flow(condition)? {
            Flow::Value(Value::Bool(true)) => self.eval_block_scoped(then_branch),
            Flow::Value(Value::Bool(false)) => self.eval_block_scoped(else_branch),
            Flow::Value(other) => type_error("bool", &other),
            flow => Ok(flow),
        }
    }

    fn eval_if_let(
        &mut self,
        pattern: &IfLetPattern,
        value: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
    ) -> RainbowResult<Flow> {
        let value = match self.eval_expr_flow(value)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        let binding = match self.eval_if_let_pattern(pattern, value)? {
            IfLetMatch::Match { binding } => binding,
            IfLetMatch::NoMatch => return self.eval_block_scoped(else_branch),
        };

        self.push_scope();
        if let Some((name, value, ty)) = binding {
            self.define(&name, value, ty, false)?;
        }
        let result = self.eval_block(then_branch);
        self.pop_scope();
        result
    }

    fn eval_if_let_pattern(
        &self,
        pattern: &IfLetPattern,
        value: Value,
    ) -> RainbowResult<IfLetMatch> {
        match pattern {
            IfLetPattern::Binding { name } => {
                if value == Value::Nil {
                    return Ok(IfLetMatch::NoMatch);
                }
                let ty = infer_value_type(&value);
                Ok(IfLetMatch::Match {
                    binding: Some((name.clone(), value, ty)),
                })
            }
            IfLetPattern::Variant {
                enum_name,
                variant,
                binding,
            } => {
                let Value::Enum {
                    name,
                    variant: value_variant,
                    payload,
                } = value
                else {
                    return Err(runtime_error(format!(
                        "if let expected enum value, found {}",
                        value.type_name()
                    )));
                };

                if name != *enum_name {
                    return Err(runtime_error(format!(
                        "if let pattern expected {name}, found {enum_name}.{variant}"
                    )));
                }

                let variants = self.enums.get(enum_name).ok_or_else(|| {
                    runtime_error(format!("if let expected enum, found {enum_name}"))
                })?;
                let Some(declared_variant) =
                    variants.iter().find(|declared| declared.name == *variant)
                else {
                    return Err(runtime_error(format!(
                        "enum '{enum_name}' has no variant '{variant}'"
                    )));
                };

                if let Some(binding) = binding {
                    let Some(payload_type) = &declared_variant.payload else {
                        return Err(runtime_error(format!(
                            "if let binding for {enum_name}.{variant} needs a payload variant"
                        )));
                    };
                    if value_variant != *variant {
                        return Ok(IfLetMatch::NoMatch);
                    }
                    let Some(payload) = payload.map(|payload| *payload) else {
                        return Err(runtime_error(format!(
                            "if let binding for {enum_name}.{variant} expected a payload value"
                        )));
                    };
                    Ok(IfLetMatch::Match {
                        binding: Some((binding.clone(), payload, payload_type.clone())),
                    })
                } else {
                    if value_variant != *variant {
                        return Ok(IfLetMatch::NoMatch);
                    }
                    Ok(IfLetMatch::Match { binding: None })
                }
            }
        }
    }

    fn eval_match(&mut self, value: &Expr, arms: &[MatchArm]) -> RainbowResult<Flow> {
        let value = match self.eval_expr_flow(value)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        let Value::Enum {
            name,
            variant,
            payload,
        } = value
        else {
            return Err(runtime_error(format!(
                "match expected an enum value, found {}",
                value.type_name()
            )));
        };

        let variants = self
            .enums
            .get(&name)
            .cloned()
            .ok_or_else(|| runtime_error(format!("match expected an enum, found {name}")))?;
        let mut seen = Vec::new();
        let mut saw_else = false;
        let mut matched_arm = None;
        let mut else_arm = None;

        for arm in arms {
            if saw_else {
                return Err(
                    runtime_error("match else arm must be last").with_fallback_span(arm.span)
                );
            }

            match &arm.pattern {
                MatchPattern::Variant {
                    enum_name,
                    variant: arm_variant,
                    binding,
                } => {
                    if enum_name != &name {
                        return Err(runtime_error(format!(
                            "match arm expected {name}, found {enum_name}.{arm_variant}"
                        ))
                        .with_fallback_span(arm.span));
                    }
                    let Some(declared) = variants
                        .iter()
                        .find(|declared| declared.name == *arm_variant)
                    else {
                        return Err(runtime_error(format!(
                            "enum '{name}' has no variant '{arm_variant}'"
                        ))
                        .with_fallback_span(arm.span));
                    };
                    if binding.is_some() && declared.payload.is_none() {
                        return Err(runtime_error(format!(
                            "match arm binding for {name}.{arm_variant} expected a payload"
                        ))
                        .with_fallback_span(arm.span));
                    }
                    if seen.iter().any(|seen| seen == arm_variant) {
                        return Err(runtime_error(format!(
                            "match has duplicate arm for {name}.{arm_variant}"
                        ))
                        .with_fallback_span(arm.span));
                    }
                    seen.push(arm_variant.clone());
                    if arm_variant == &variant {
                        matched_arm = Some((arm, declared.payload.clone()));
                    }
                }
                MatchPattern::Else => {
                    saw_else = true;
                    else_arm = Some(arm);
                }
            }
        }

        if else_arm.is_none() {
            for variant in variants {
                if !seen.iter().any(|seen| seen == &variant.name) {
                    return Err(runtime_error(format!(
                        "match missing arm for {name}.{}",
                        variant.name
                    )));
                }
            }
        }

        if let Some((arm, payload_type)) = matched_arm {
            return self.eval_match_arm(arm, payload_type.as_ref(), payload.as_deref());
        }

        if let Some(arm) = else_arm {
            return self.eval_block_scoped(&arm.body);
        }

        Err(runtime_error(format!(
            "match missing arm for {name}.{variant}"
        )))
    }

    fn eval_match_arm(
        &mut self,
        arm: &MatchArm,
        payload_type: Option<&TypeName>,
        payload: Option<&Value>,
    ) -> RainbowResult<Flow> {
        let MatchPattern::Variant { binding, .. } = &arm.pattern else {
            return self.eval_block_scoped(&arm.body);
        };
        let Some(binding) = binding else {
            return self.eval_block_scoped(&arm.body);
        };
        let Some(payload_type) = payload_type else {
            return Err(
                runtime_error("match arm binding expected a payload").with_fallback_span(arm.span)
            );
        };
        let Some(payload) = payload else {
            return Err(runtime_error("match arm binding expected a payload value")
                .with_fallback_span(arm.span));
        };

        self.push_scope();
        self.define(binding, payload.clone(), payload_type.clone(), false)?;
        let result = self.eval_block(&arm.body);
        self.pop_scope();
        result
    }

    fn eval_while(&mut self, condition: &Expr, body: &[Statement]) -> RainbowResult<Flow> {
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

    fn eval_for(&mut self, name: &str, iterable: &Expr, body: &[Statement]) -> RainbowResult<Flow> {
        let iterable = match self.eval_expr_flow(iterable)? {
            Flow::Value(value) => value,
            flow => return Ok(flow),
        };

        let values = match iterable {
            Value::Array(values) => values,
            Value::Str(value) => value
                .chars()
                .map(|ch| Value::Str(ch.to_string()))
                .collect::<Vec<_>>(),
            other => {
                return Err(runtime_error(format!(
                    "for loop expected an array or str, found {}",
                    other.type_name()
                )));
            }
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

    fn eval_block_scoped(&mut self, statements: &[Statement]) -> RainbowResult<Flow> {
        self.push_scope();
        let result = self.eval_block(statements);
        self.pop_scope();
        result
    }

    fn eval_block(&mut self, statements: &[Statement]) -> RainbowResult<Flow> {
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
    ) -> RainbowResult<()> {
        let ty = if *annotation == TypeName::Infer {
            if value == Value::Nil {
                return Err(runtime_error(format!(
                    "binding '{name}' needs an explicit nullable type for nil"
                )));
            }
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

    fn define(
        &mut self,
        name: &str,
        value: Value,
        ty: TypeName,
        mutable: bool,
    ) -> RainbowResult<()> {
        if self.structs.contains_key(name) || self.enums.contains_key(name) {
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

    fn assign(&mut self, name: &str, value: Value) -> RainbowResult<()> {
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
            Value::Float(_) => "f64",
            Value::Bool(_) => "bool",
            Value::Str(_) => "str",
            Value::Nil => "nil",
            Value::Array(_) => "array",
            Value::Struct { .. } => "struct",
            Value::Enum { .. } => "enum",
            Value::Function(_) => "fn",
            Value::Unit => "unit",
        }
    }
}

fn type_error<T>(expected: &str, actual: &Value) -> RainbowResult<T> {
    Err(runtime_error(format!(
        "expected {expected}, found {}",
        actual.type_name()
    )))
}

fn reject_inferred_signature(
    name: &str,
    params: &[Param],
    return_type: &TypeName,
) -> RainbowResult<()> {
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
) -> RainbowResult<()> {
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
        Value::Float(_) => TypeName::F64,
        Value::Bool(_) => TypeName::Bool,
        Value::Str(_) => TypeName::Str,
        Value::Nil => TypeName::Infer,
        Value::Unit => TypeName::Unit,
        Value::Struct { name, .. } => TypeName::Struct(name.clone()),
        Value::Enum { name, .. } => TypeName::Struct(name.clone()),
        Value::Array(values) => TypeName::Array(Box::new(infer_array_element_type(values))),
        Value::Function(_) => TypeName::Infer,
    }
}

fn infer_array_element_type(values: &[Value]) -> TypeName {
    array_element_type(values).unwrap_or(TypeName::Infer)
}

fn value_matches_type(value: &Value, ty: &TypeName) -> bool {
    match ty {
        TypeName::Infer => true,
        TypeName::I64 => matches!(value, Value::Int(_)),
        TypeName::F64 => matches!(value, Value::Float(_)),
        TypeName::Bool => matches!(value, Value::Bool(_)),
        TypeName::Str => matches!(value, Value::Str(_)),
        TypeName::Unit => matches!(value, Value::Unit),
        TypeName::Struct(expected) => {
            matches!(value, Value::Struct { name, .. } | Value::Enum { name, .. } if name == expected)
        }
        TypeName::Array(element) => match value {
            Value::Array(values) => values
                .iter()
                .all(|value| value_matches_type(value, element)),
            _ => false,
        },
        TypeName::Nullable(inner) => {
            matches!(value, Value::Nil) || value_matches_type(value, inner)
        }
    }
}

fn format_type_name(ty: &TypeName) -> String {
    match ty {
        TypeName::Infer => "infer".to_owned(),
        TypeName::I64 => "i64".to_owned(),
        TypeName::F64 => "f64".to_owned(),
        TypeName::Bool => "bool".to_owned(),
        TypeName::Str => "str".to_owned(),
        TypeName::Unit => "unit".to_owned(),
        TypeName::Struct(name) => name.clone(),
        TypeName::Array(element) => format!("[{}]", format_type_name(element)),
        TypeName::Nullable(inner) => format!("{}?", format_type_name(inner)),
    }
}

fn format_value_type(value: &Value) -> String {
    match value {
        Value::Array(values) => format_array_type(values),
        Value::Struct { name, .. } => name.clone(),
        Value::Enum { name, .. } => name.clone(),
        _ => value.type_name().to_owned(),
    }
}

fn format_array_type(values: &[Value]) -> String {
    array_element_type(values)
        .map(|ty| format!("[{}]", format_type_name(&ty)))
        .unwrap_or_else(|| "array".to_owned())
}

fn ensure_homogeneous_array(values: &[Value], context: &str) -> RainbowResult<()> {
    let Some(expected) = array_element_type(values) else {
        return Ok(());
    };

    for value in values {
        if !value_matches_type(value, &expected) {
            return Err(runtime_error(format!(
                "{context} expected {}, found {}",
                format_type_name(&expected),
                format_value_type(value)
            )));
        }
    }

    Ok(())
}

fn ensure_array_concat_matches(left: &[Value], right: &Value) -> RainbowResult<()> {
    let Value::Array(right) = right else {
        unreachable!("array concatenation only calls this helper for arrays")
    };

    ensure_homogeneous_array(left, "array element")?;
    ensure_homogeneous_array(right, "array element")?;

    match (array_element_type(left), array_element_type(right)) {
        (Some(expected), Some(_)) => {
            for value in right {
                if !value_matches_type(value, &expected) {
                    return Err(runtime_error(format!(
                        "array element expected {}, found {}",
                        format_type_name(&expected),
                        format_value_type(value)
                    )));
                }
            }
        }
        (None, Some(expected)) => {
            for value in left {
                if !value_matches_type(value, &expected) {
                    return Err(runtime_error(format!(
                        "array element expected {}, found {}",
                        format_type_name(&expected),
                        format_value_type(value)
                    )));
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn ensure_array_item_matches(values: &[Value], value: &Value, context: &str) -> RainbowResult<()> {
    let Some(expected) = array_element_type(values) else {
        return Ok(());
    };

    if value_matches_type(value, &expected) {
        return Ok(());
    }

    Err(runtime_error(format!(
        "{context} expected {}, found {}",
        format_type_name(&expected),
        format_value_type(value)
    )))
}

fn ensure_array_fallback_matches(
    values: &[Value],
    value: &Value,
    context: &str,
) -> RainbowResult<()> {
    ensure_array_item_matches(values, value, context)
}

fn expect_string_array(values: Vec<Value>, context: &str) -> RainbowResult<Vec<String>> {
    let mut strings = Vec::with_capacity(values.len());

    for value in values {
        match value {
            Value::Str(value) => strings.push(value),
            other => {
                return Err(runtime_error(format!(
                    "{context} expected [str], found {}",
                    format_value_type(&Value::Array(vec![other]))
                )));
            }
        }
    }

    Ok(strings)
}

fn array_element_type(values: &[Value]) -> Option<TypeName> {
    let has_nil = values.iter().any(|value| matches!(value, Value::Nil));
    let element = values
        .iter()
        .map(infer_value_type)
        .find(|ty| !type_has_infer(ty))?;

    if has_nil {
        Some(TypeName::Nullable(Box::new(element)))
    } else {
        Some(element)
    }
}

fn type_has_infer(ty: &TypeName) -> bool {
    match ty {
        TypeName::Infer => true,
        TypeName::Array(element) => type_has_infer(element),
        TypeName::Nullable(inner) => type_has_infer(inner),
        TypeName::I64
        | TypeName::F64
        | TypeName::Bool
        | TypeName::Str
        | TypeName::Unit
        | TypeName::Struct(_) => false,
    }
}

fn checked_int(operation: &str, value: Option<i64>) -> RainbowResult<Value> {
    value
        .map(Value::Int)
        .ok_or_else(|| runtime_error(format!("integer overflow in {operation}")))
}

fn checked_float(operation: &str, value: f64) -> RainbowResult<Value> {
    if value.is_finite() {
        Ok(Value::Float(value))
    } else {
        Err(runtime_error(format!(
            "non-finite floating-point result in {operation}"
        )))
    }
}

fn format_float(value: f64) -> String {
    let mut raw = value.to_string();
    if !raw.contains('.') && !raw.contains('e') && !raw.contains('E') {
        raw.push_str(".0");
    }
    raw
}

fn checked_slice_bounds(start: i64, end: i64, len: usize) -> RainbowResult<(usize, usize)> {
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

fn values_equal(left: &Value, right: &Value) -> RainbowResult<bool> {
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

fn runtime_error(message: impl Into<String>) -> RainbowError {
    RainbowError::new(message, Span::new(0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn run(source: &str) -> RainbowResult<RunResult> {
        let tokens = lex(source)?;
        let program = parse(&tokens)?;
        Evaluator::new().run(&program)
    }

    #[test]
    fn runtime_errors_use_statement_spans() {
        let error = run(r#"
1 / 0
"#)
        .expect_err("division by zero should fail");

        assert!(error.message.contains("division by zero"));
        assert_eq!((error.line, error.column), (2, 1));
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
    fn supports_nullable_values_at_runtime() {
        let result = run(r#"
let missing: i64? = nil
let present: i64? = 7
var current: i64? = missing
current = 42
let values: [i64?] = [nil, present, current]
let fallback = missing ?? 11
let chosen = present ?? 99
assert(type(missing) == "nil")
assert(missing == nil)
assert(present != nil)
assert(contains(values, nil))
assert(find(values, 42) == 2)
assert(fallback == 11)
chosen + current
"#)
        .expect("nullable runtime program should run");

        assert_eq!(result.last_value, Value::Int(49));
    }

    #[test]
    fn coalesce_short_circuits_present_values() {
        let result = run(r#"
fn explode() -> i64:
    return 1 / 0

let present: i64? = 7
let missing: i64? = nil
let chosen = present ?? explode()
let recovered = missing ?? 35
chosen + recovered
"#)
        .expect("present coalesce should skip fallback");

        assert_eq!(result.last_value, Value::Int(42));
    }

    #[test]
    fn supports_if_let_nullable_unwrapping() {
        let result = run(r#"
fn maybe(flag: bool) -> i64?:
    if flag:
        return 41
    else:
        return nil

let recovered = if let value = maybe(true):
    value + 1
else:
    0

var seen = 0
if let missing = maybe(false):
    seen = missing
elif let fallback = maybe(true):
    seen = fallback
else:
    seen = -1

assert(recovered == 42)

enum Result:
    Ok(i64)
    Err(str)

let ok = Result.Ok(41)
let enum_recovered = if let Result.Ok(value) = ok:
    value + 1
else:
    0

if let Result.Err(message) = ok:
    seen = len(message)
elif let Result.Ok(value) = ok:
    seen = value
else:
    seen = -1

assert(enum_recovered == 42)
seen + enum_recovered
"#)
        .expect("if let should unwrap nullable values and enum variants");

        assert_eq!(result.last_value, Value::Int(83));
    }

    #[test]
    fn rejects_runtime_if_let_enum_pattern_errors() {
        let non_enum = run(r#"
enum Result:
    Ok(i64)

if let Result.Ok(value) = 42:
    print(value)
"#)
        .expect_err("if let enum pattern on non-enum runtime value should fail");
        assert!(
            non_enum
                .message
                .contains("if let expected enum value, found i64")
        );

        let wrong_enum = run(r#"
enum Result:
    Ok(i64)

enum Status:
    Ready

if let Status.Ready = Result.Ok(1):
    print("ready")
"#)
        .expect_err("if let wrong enum runtime pattern should fail");
        assert!(
            wrong_enum
                .message
                .contains("if let pattern expected Result, found Status.Ready")
        );

        let unit_binding = run(r#"
enum Status:
    Ready

if let Status.Ready(value) = Status.Ready:
    print(value)
"#)
        .expect_err("if let binding on unit runtime variant should fail");
        assert!(
            unit_binding
                .message
                .contains("if let binding for Status.Ready needs a payload variant")
        );
    }

    #[test]
    fn rejects_untyped_runtime_nil_bindings() {
        let error = run("let missing = nil\n").expect_err("untyped nil should fail at runtime");

        assert!(error.message.contains("explicit nullable type"));
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
        let result = run("\"Rain\" + \"bow\"\n").expect("program should run");

        assert_eq!(result.last_value, Value::Str("Rainbow".to_owned()));
    }

    #[test]
    fn supports_f64_arithmetic_and_comparison() {
        let result = run(r#"
fn average(total: f64, count: f64) -> f64:
    return total / count

let radius: f64 = 2.5
let area = 3.14 * radius * radius
let shifted = -area + 20.0
let maybe: f64? = nil
let recovered = maybe ?? average(7.5, 3.0)
assert(area > 19.6 and area < 19.7)
assert(shifted > 0.3)
assert(recovered == 2.5)
recovered
"#)
        .expect("f64 arithmetic should run");

        assert_eq!(result.last_value, Value::Float(2.5));
    }

    #[test]
    fn supports_explicit_numeric_conversions() {
        let result = run(r#"
let count: i64 = 4
let total: f64 = f64(count) + 2.5
let whole: i64 = i64(total - 0.5)
assert(type(total) == "f64")
assert(type(whole) == "i64")
whole
"#)
        .expect("numeric conversions should run");

        assert_eq!(result.last_value, Value::Int(6));
    }

    #[test]
    fn rejects_unsafe_numeric_conversions() {
        let fractional =
            run("i64(2.5)\n").expect_err("fractional f64 to i64 should fail at runtime");
        assert!(
            fractional
                .message
                .contains("i64 conversion expected a whole f64 value")
        );

        let precision = run("f64(9007199254740993)\n")
            .expect_err("precision-losing i64 to f64 should fail at runtime");
        assert!(
            precision
                .message
                .contains("f64 conversion would lose integer precision")
        );
    }

    #[test]
    fn rejects_division_by_zero() {
        let error = run("1 / 0\n").expect_err("division by zero should fail");

        assert!(error.message.contains("division by zero"));
    }

    #[test]
    fn rejects_f64_division_and_remainder_by_zero() {
        let division = run("1.0 / 0.0\n").expect_err("float division by zero should fail");
        assert!(division.message.contains("division by zero"));

        let remainder = run("1.0 % 0.0\n").expect_err("float remainder by zero should fail");
        assert!(remainder.message.contains("remainder by zero"));
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
    fn supports_enum_variants_and_equality() {
        let result = run(r#"
let status = Status.Ready

enum Status:
    Pending
    Ready

let history: [Status] = [Status.Pending, status]
assert(status == Status.Ready)
assert(status != Status.Pending)
assert(contains(history, Status.Pending))
assert(type(status) == "Status")
assert(type(history) == "[Status]")
print(status)
status
"#)
        .expect("enum program should run");

        assert_eq!(
            result.last_value,
            Value::Enum {
                name: "Status".to_owned(),
                variant: "Ready".to_owned(),
                payload: None,
            }
        );
        assert_eq!(result.outputs, vec!["Status.Ready"]);
    }

    #[test]
    fn supports_enum_match_expressions() {
        let result = run(r#"
enum Status:
    Pending
    Ready
    Failed

fn describe(status: Status) -> str:
    return match status:
        Status.Pending:
            "pending"
        Status.Ready:
            "ready"
        else:
            "failed"

let ready = describe(Status.Ready)
let failed = describe(Status.Failed)
assert(ready == "ready")
assert(failed == "failed")
ready
"#)
        .expect("enum match should run");

        assert_eq!(result.last_value, Value::Str("ready".to_owned()));
    }

    #[test]
    fn supports_payload_enum_variants_and_match_bindings() {
        let result = run(r#"
enum Result:
    Ok(i64)
    Err(str)

fn unwrap_or_len(result: Result) -> i64:
    return match result:
        Result.Ok(value):
            value + 1
        Result.Err(message):
            len(message)

let ok = Result.Ok(41)
let err = Result.Err("oops")
let recovered = if let Result.Ok(value) = ok:
    value + 1
else:
    0
let missed = if let Result.Err(message) = ok:
    len(message)
else:
    0
assert(ok == Result.Ok(41))
assert(ok != Result.Ok(42))
assert(type(ok) == "Result")
assert(unwrap_or_len(ok) == 42)
assert(unwrap_or_len(err) == 4)
assert(recovered == 42)
assert(missed == 0)
print(ok)
ok
"#)
        .expect("payload enum program should run");

        assert_eq!(
            result.last_value,
            Value::Enum {
                name: "Result".to_owned(),
                variant: "Ok".to_owned(),
                payload: Some(Box::new(Value::Int(41))),
            }
        );
        assert_eq!(result.outputs, vec!["Result.Ok(41)"]);
    }

    #[test]
    fn rejects_runtime_enum_match_arm_errors() {
        let missing = run(r#"
enum Status:
    Pending
    Ready

match Status.Ready:
    Status.Ready:
        "ready"
"#)
        .expect_err("missing runtime match arm should fail");
        assert!(
            missing
                .message
                .contains("match missing arm for Status.Pending")
        );

        let duplicate = run(r#"
enum Status:
    Ready

match Status.Ready:
    Status.Ready:
        "ready"
    Status.Ready:
        "again"
"#)
        .expect_err("duplicate runtime match arm should fail");
        assert!(
            duplicate
                .message
                .contains("match has duplicate arm for Status.Ready")
        );
        assert_eq!((duplicate.line, duplicate.column), (8, 5));

        let wrong_enum = run(r#"
enum Status:
    Ready

enum Mode:
    Ready

match Status.Ready:
    Mode.Ready:
        "ready"
    else:
        "other"
"#)
        .expect_err("wrong runtime enum match arm should fail");
        assert!(
            wrong_enum
                .message
                .contains("match arm expected Status, found Mode.Ready")
        );
        assert_eq!((wrong_enum.line, wrong_enum.column), (9, 5));

        let unit_binding = run(r#"
enum Status:
    Ready

match Status.Ready:
    Status.Ready(value):
        value
"#)
        .expect_err("unit runtime match binding should fail");
        assert!(
            unit_binding
                .message
                .contains("match arm binding for Status.Ready expected a payload")
        );

        let missing_payload = run(r#"
enum Result:
    Ok(i64)

Result.Ok
"#)
        .expect_err("missing runtime enum payload should fail");
        assert!(
            missing_payload
                .message
                .contains("enum variant 'Result.Ok' needs a payload")
        );

        let unit_call = run(r#"
enum Status:
    Ready

Status.Ready()
"#)
        .expect_err("unit runtime enum call should fail");
        assert!(
            unit_call
                .message
                .contains("enum variant 'Status.Ready' is a value and should not be called")
        );

        let unit_if_let_binding = run(r#"
enum Status:
    Pending
    Ready

if let Status.Ready(value) = Status.Pending:
    print(value)
"#)
        .expect_err("unit runtime if let binding should fail");
        assert!(
            unit_if_let_binding
                .message
                .contains("if let binding for Status.Ready needs a payload variant")
        );
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
    fn supports_string_indexing() {
        let result = run(r#"
let name = "Rainbow"
assert(name[0] == "R")
assert(name[1] == "a")
assert(name[2] == "i")
"#)
        .expect("string indexing should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_string_standard_library() {
        let result = run(r#"
let phrase = "  Fast Secure Simple  "
let trimmed = trim(phrase)
let words = split(lower(trimmed), " ")
let joined = join(words, "-")

assert(trimmed == "Fast Secure Simple")
assert(lower("RAINBOW") == "rainbow")
assert(upper("rainbow") == "RAINBOW")
assert(starts_with(trimmed, "Fast"))
assert(ends_with(trimmed, "Simple"))
assert(replace(trimmed, "Simple", "Readable") == "Fast Secure Readable")
assert(words == ["fast", "secure", "simple"])
assert(joined == "fast-secure-simple")
assert(join([], ",") == "")
"#)
        .expect("string standard library should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_flow_calls() {
        let result = run(r#"
fn bracket(value: str, left: str, right: str) -> str:
    return left + value + right

let label = "  Rainbow  " then trim then lower then bracket("[", "]")
assert(label == "[rainbow]")
assert((label then contains("rainbow")))
label then len
"#)
        .expect("flow calls should run");

        assert_eq!(result.last_value, Value::Int(9));
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
    fn runs_for_loop_over_string() {
        let result = run(r#"
var seen = ""
for ch in "Rainbow":
    seen = seen + ch

seen
"#)
        .expect("string for loop should run");

        assert_eq!(result.last_value, Value::Str("Rainbow".to_owned()));
    }

    #[test]
    fn supports_control_flow_in_string_for_loop() {
        let result = run(r#"
var seen = ""
for ch in "Rainbow!":
    if ch == "a":
        continue
    if ch == "!":
        break
    seen = seen + ch

seen
"#)
        .expect("string for loop control flow should run");

        assert_eq!(result.last_value, Value::Str("Rinbow".to_owned()));
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
assert(contains("secure Rainbow", "Rainbow"))
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
assert(slice("secure Rainbow", 0, 6) == "secure")
assert(slice("Rainbow", 1, 4) == "ain")
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
assert(not is_empty("Rainbow"))
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
assert(get("Rainbow", 1, "?") == "a")
assert(get("Rainbow", 9, "?") == "?")
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
assert(find("secure Rainbow", "Rainbow") == 7)
assert(find("secure Rainbow", "missing") == -1)
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
assert(count("secure Rainbow secure", "secure") == 2)
assert(count("aaaa", "aa") == 2)
assert(count("secure Rainbow", "missing") == 0)
assert(count("Rainbow", "") == 0)
"#)
        .expect("count should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn supports_reverse_first_and_last_for_arrays_and_strings() {
        let result = run(r#"
let values = [3, 5, 8]
assert(reverse(values) == [8, 5, 3])
assert(first(values, -1) == 3)
assert(last(values, -1) == 8)
assert(first([], 42) == 42)
assert(last([], 42) == 42)
assert(reverse("Rainbow") == "wobniaR")
assert(first("Rainbow", "?") == "R")
assert(last("Rainbow", "?") == "w")
assert(first("", "?") == "?")
assert(last("", "?") == "?")
"#)
        .expect("reverse/first/last should run");

        assert_eq!(result.last_value, Value::Unit);
    }

    #[test]
    fn rejects_find_runtime_type_errors() {
        let collection = run("find(42, 1)\n").expect_err("find collection should fail");
        assert!(collection.message.contains("find expects an array or str"));

        let needle = run("find(\"Rainbow\", 1)\n").expect_err("find string needle should fail");
        assert!(needle.message.contains("find(str, value) expected str"));
    }

    #[test]
    fn rejects_count_runtime_type_errors() {
        let collection = run("count(42, 1)\n").expect_err("count collection should fail");
        assert!(collection.message.contains("count expects an array or str"));

        let needle = run("count(\"Rainbow\", 1)\n").expect_err("count string needle should fail");
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

        let too_far = run("slice(\"Rainbow\", 0, 8)\n").expect_err("oversized end should fail");
        assert!(too_far.message.contains("slice end 8 out of bounds"));
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
        let error = run("contains(\"rainbow\", 1)\n").expect_err("contains should fail");

        assert!(error.message.contains("contains(str, value) expected str"));
    }

    #[test]
    fn rejects_array_runtime_element_mismatches() {
        let literal = run("[1, true]\n").expect_err("mixed array literal should fail");
        assert!(literal.message.contains("array element expected i64"));

        let nested = run("[[1], [true]]\n").expect_err("mixed nested array should fail");
        assert!(nested.message.contains("array element expected [i64]"));

        let concat = run("[1] + [true]\n").expect_err("mixed concat should fail");
        assert!(concat.message.contains("array element expected i64"));

        let append = run("append([1], true)\n").expect_err("mixed append should fail");
        assert!(append.message.contains("append expected i64"));

        let contains = run("contains([1], true)\n").expect_err("mixed contains should fail");
        assert!(contains.message.contains("contains expected i64"));

        let find = run("find([1], true)\n").expect_err("mixed find should fail");
        assert!(find.message.contains("find expected i64"));

        let count = run("count([1], true)\n").expect_err("mixed count should fail");
        assert!(count.message.contains("count expected i64"));

        let fallback = run("get([1], 3, true)\n").expect_err("mixed get fallback should fail");
        assert!(fallback.message.contains("get default expected i64"));
    }

    #[test]
    fn rejects_reverse_first_and_last_runtime_type_errors() {
        let reverse = run("reverse(42)\n").expect_err("reverse collection should fail");
        assert!(reverse.message.contains("reverse expects an array or str"));

        let first = run("first(42, 0)\n").expect_err("first collection should fail");
        assert!(first.message.contains("first expects an array or str"));

        let last = run("last(42, 0)\n").expect_err("last collection should fail");
        assert!(last.message.contains("last expects an array or str"));

        let first_default = run("first(\"\", 0)\n").expect_err("first default should fail");
        assert!(first_default.message.contains("first default expected str"));

        let last_default = run("last(\"\", 0)\n").expect_err("last default should fail");
        assert!(last_default.message.contains("last default expected str"));

        let get_default = run("get(\"Rainbow\", 9, 0)\n").expect_err("get default should fail");
        assert!(get_default.message.contains("get default expected str"));
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
    fn runs_branch_empty_arrays_with_expected_types() {
        let result = run(r#"
enum Source:
    Empty
    Full

let flag = true
let from_if: [i64] = if flag:
    []
else:
    [1, 2]

let maybe: i64? = nil
let from_if_let: [i64] = if let value = maybe:
    [value]
else:
    []

let from_match: [i64] = match Source.Empty:
    Source.Empty:
        []
    Source.Full:
        [3, 5]

len(from_if) + len(from_if_let) + len(from_match)
"#)
        .expect("branch empty arrays should use annotated result types");

        assert_eq!(result.last_value, Value::Int(0));
    }

    #[test]
    fn runs_empty_array_branches_with_sibling_type_inference() {
        let result = run(r#"
enum Source:
    Empty
    Full

let flag = true
let from_if = if flag:
    []
else:
    [1, 2]

let from_reverse = if flag:
    reverse([])
else:
    [8, 13]

let maybe: i64? = nil
let from_if_let = if let value = maybe:
    [value]
else:
    []

let from_match = match Source.Empty:
    Source.Empty:
        []
    Source.Full:
        [3, 5]

len(from_if) + len(from_reverse) + len(from_if_let) + len(from_match)
"#)
        .expect("empty branches should infer array type from sibling branches");

        assert_eq!(result.last_value, Value::Int(0));
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

    #[test]
    fn rejects_string_index_runtime_errors() {
        let non_integer = run("\"Rainbow\"[true]\n").expect_err("string index type should fail");
        assert!(non_integer.message.contains("string index expected i64"));

        let negative = run("\"Rainbow\"[-1]\n").expect_err("negative string index should fail");
        assert!(negative.message.contains("string index -1 out of bounds"));

        let too_far = run("\"Rainbow\"[7]\n").expect_err("oversized string index should fail");
        assert!(too_far.message.contains("string index 7 out of bounds"));

        let collection = run("42[0]\n").expect_err("indexing primitive should fail");
        assert!(
            collection
                .message
                .contains("indexing expected an array or str")
        );
    }

    #[test]
    fn rejects_string_standard_library_runtime_errors() {
        let transform = run("trim(42)\n").expect_err("trim type should fail");
        assert!(transform.message.contains("trim expected str"));

        let predicate =
            run("starts_with(\"Rainbow\", 1)\n").expect_err("starts_with value should fail");
        assert!(predicate.message.contains("starts_with value expected str"));

        let join_collection = run("join(42, \",\")\n").expect_err("join collection should fail");
        assert!(join_collection.message.contains("join expects [str]"));

        let join_item = run("join([1], \",\")\n").expect_err("join item should fail");
        assert!(join_item.message.contains("join expected [str]"));

        let split_empty =
            run("split(\"Rainbow\", \"\")\n").expect_err("split separator should fail");
        assert!(
            split_empty
                .message
                .contains("split separator must not be empty")
        );

        let replace_empty =
            run("replace(\"Rainbow\", \"\", \"-\")\n").expect_err("replace old should fail");
        assert!(
            replace_empty
                .message
                .contains("replace old value must not be empty")
        );
    }
}

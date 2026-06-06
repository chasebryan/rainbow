use std::collections::HashMap;
use std::fmt::{Display, Formatter};

use crate::ast::{BinaryOp, Expr, Param, Program, Statement, TypeName, UnaryOp};
use crate::diagnostic::{FyrError, FyrResult};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Infer,
    Never,
    I64,
    Bool,
    Str,
    Unit,
    Struct(String),
    Array(Box<Type>),
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Infer => write!(f, "infer"),
            Type::Never => write!(f, "never"),
            Type::I64 => write!(f, "i64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Unit => write!(f, "unit"),
            Type::Struct(name) => write!(f, "{name}"),
            Type::Array(element) => write!(f, "[{element}]"),
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
    structs: HashMap<String, Vec<Param>>,
    return_types: Vec<Type>,
    loop_depth: usize,
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
            structs: HashMap::new(),
            return_types: Vec::new(),
            loop_depth: 0,
        }
    }

    fn check_program(mut self, program: &Program) -> FyrResult<()> {
        self.predeclare_structs(&program.statements)?;
        self.predeclare_functions(&program.statements)?;

        for statement in &program.statements {
            self.check_statement(statement)?;
        }

        Ok(())
    }

    fn predeclare_structs(&mut self, statements: &[Statement]) -> FyrResult<()> {
        for statement in statements {
            if let Statement::Struct { name, fields } = statement {
                if self.structs.contains_key(name) || self.current_scope().contains_key(name) {
                    return Err(type_error(format!("struct '{name}' already exists")));
                }

                self.structs.insert(name.clone(), fields.clone());
            }
        }

        for fields in self.structs.values() {
            for field in fields {
                self.validate_type_name(&field.ty)?;
            }
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
                for param in params {
                    self.validate_type_name(&param.ty)?;
                }
                self.validate_type_name(return_type)?;

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
            Statement::Struct { .. } => Ok(Type::Unit),
            Statement::Let { name, ty, value } => self.check_binding(name, ty, value, false),
            Statement::Var { name, ty, value } => self.check_binding(name, ty, value, true),
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
                for param in params {
                    self.validate_type_name(&param.ty)?;
                }
                self.validate_type_name(return_type)?;

                let expected = return_type.as_type();
                self.push_scope();
                self.return_types.push(expected.clone());
                for Param { name, ty } in params {
                    self.define(name, ty.as_type(), false);
                }
                let body_type = self.check_block(body)?;
                self.return_types.pop();
                self.pop_scope();

                if body_type != expected && body_type != Type::Never {
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
            Statement::For {
                name,
                iterable,
                body,
            } => {
                self.check_for(name, iterable, body)?;
                Ok(Type::Unit)
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
            } => self.check_if_statement(condition, then_branch, else_branch),
            Statement::Return { value } => self.check_return(value.as_ref()),
            Statement::Break => {
                if self.loop_depth == 0 {
                    return Err(type_error("break outside loop"));
                }
                Ok(Type::Never)
            }
            Statement::Continue => {
                if self.loop_depth == 0 {
                    return Err(type_error("continue outside loop"));
                }
                Ok(Type::Never)
            }
            Statement::Expr(expr) => self.check_expr(expr),
        }
    }

    fn check_binding(
        &mut self,
        name: &str,
        annotation: &TypeName,
        value: &Expr,
        mutable: bool,
    ) -> FyrResult<Type> {
        let expected = if *annotation == TypeName::Infer {
            None
        } else {
            self.validate_type_name(annotation)?;
            Some(annotation.as_type())
        };

        let value_type = self.check_expr_with_hint(value, expected.as_ref())?;
        let binding_type = if let Some(expected) = expected {
            if value_type != expected {
                return Err(type_error(format!(
                    "binding '{name}' expected {expected}, found {value_type}"
                )));
            }
            expected
        } else {
            value_type
        };

        self.define(name, binding_type, mutable);
        Ok(Type::Unit)
    }

    fn check_expr(&mut self, expr: &Expr) -> FyrResult<Type> {
        self.check_expr_with_hint(expr, None)
    }

    fn check_expr_with_hint(&mut self, expr: &Expr, expected: Option<&Type>) -> FyrResult<Type> {
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
            Expr::StructInit { name, fields } => self.check_struct_init(name, fields),
            Expr::Field { object, field } => self.check_field(object, field),
            Expr::Array(elements) => self.check_array(elements, expected),
            Expr::Index { collection, index } => self.check_index(collection, index),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.check_if(condition, then_branch, else_branch),
        }
    }

    fn check_array(&mut self, elements: &[Expr], expected: Option<&Type>) -> FyrResult<Type> {
        let expected_element = match expected {
            Some(Type::Array(element)) => Some(element.as_ref()),
            _ => None,
        };

        let Some(first) = elements.first() else {
            if let Some(element) = expected_element {
                return Ok(Type::Array(Box::new(element.clone())));
            }
            return Err(type_error("empty array literals need an element type"));
        };

        let element_type = self.check_expr_with_hint(first, expected_element)?;
        for element in elements.iter().skip(1) {
            let found = self.check_expr_with_hint(element, expected_element)?;
            if found != element_type {
                return Err(type_error(format!(
                    "array element expected {element_type}, found {found}"
                )));
            }
        }

        if let Some(expected_element) = expected_element {
            if element_type != *expected_element {
                return Err(type_error(format!(
                    "array element expected {expected_element}, found {element_type}"
                )));
            }
        }

        Ok(Type::Array(Box::new(element_type)))
    }

    fn check_index(&mut self, collection: &Expr, index: &Expr) -> FyrResult<Type> {
        let collection_type = self.check_expr(collection)?;
        let Type::Array(element_type) = collection_type else {
            return Err(type_error(format!(
                "indexing expected an array, found {collection_type}"
            )));
        };

        let index_type = self.check_expr(index)?;
        if index_type != Type::I64 {
            return Err(type_error(format!(
                "array index expected i64, found {index_type}"
            )));
        }

        Ok(*element_type)
    }

    fn check_struct_init(&mut self, name: &str, fields: &[(String, Expr)]) -> FyrResult<Type> {
        let declared_fields = self
            .structs
            .get(name)
            .cloned()
            .ok_or_else(|| type_error(format!("unknown struct '{name}'")))?;

        let mut seen = HashMap::new();
        for (field_name, value) in fields {
            if seen.insert(field_name.clone(), ()).is_some() {
                return Err(type_error(format!(
                    "field '{field_name}' initialized more than once"
                )));
            }

            let Some(field) = declared_fields
                .iter()
                .find(|field| field.name == *field_name)
            else {
                return Err(type_error(format!(
                    "struct '{name}' has no field '{field_name}'"
                )));
            };

            let expected = field.ty.as_type();
            let found = self.check_expr(value)?;
            if found != expected {
                return Err(type_error(format!(
                    "field '{field_name}' expected {expected}, found {found}"
                )));
            }
        }

        for field in &declared_fields {
            if !seen.contains_key(&field.name) {
                return Err(type_error(format!(
                    "struct '{name}' missing field '{}'",
                    field.name
                )));
            }
        }

        Ok(Type::Struct(name.to_owned()))
    }

    fn check_field(&mut self, object: &Expr, field_name: &str) -> FyrResult<Type> {
        let object_type = self.check_expr(object)?;
        let Type::Struct(struct_name) = object_type else {
            return Err(type_error(format!(
                "field access expected a struct, found {object_type}"
            )));
        };

        let fields = self
            .structs
            .get(&struct_name)
            .ok_or_else(|| type_error(format!("unknown struct '{struct_name}'")))?;
        let field = fields
            .iter()
            .find(|field| field.name == field_name)
            .ok_or_else(|| {
                type_error(format!(
                    "struct '{struct_name}' has no field '{field_name}'"
                ))
            })?;

        Ok(field.ty.as_type())
    }

    fn check_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> FyrResult<Type> {
        if op == BinaryOp::Add {
            return self.check_add(left, right);
        }

        let left_type = self.check_expr(left)?;
        let right_type = self.check_expr(right)?;

        match op {
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
            BinaryOp::Equal | BinaryOp::NotEqual
                if left_type == right_type && is_equatable_type(&left_type) =>
            {
                Ok(Type::Bool)
            }
            BinaryOp::Equal | BinaryOp::NotEqual if left_type == right_type => Err(type_error(
                format!("{left_type} cannot be compared for equality"),
            )),
            BinaryOp::And | BinaryOp::Or if left_type == Type::Bool && right_type == Type::Bool => {
                Ok(Type::Bool)
            }
            _ => Err(type_error(format!(
                "operator '{op:?}' cannot be applied to {left_type} and {right_type}"
            ))),
        }
    }

    fn check_add(&mut self, left: &Expr, right: &Expr) -> FyrResult<Type> {
        let left_type = if is_empty_array_literal(left) {
            None
        } else {
            Some(self.check_expr(left)?)
        };

        let Some(left_type) = left_type else {
            let right_type = self.check_expr(right)?;
            if matches!(right_type, Type::Array(_)) {
                self.check_expr_with_hint(left, Some(&right_type))?;
                return Ok(right_type);
            }

            return Err(type_error("empty array literals need an element type"));
        };

        match &left_type {
            Type::I64 => {
                let right_type = self.check_expr(right)?;
                if right_type == Type::I64 {
                    Ok(Type::I64)
                } else {
                    Err(type_error(format!(
                        "operator 'Add' cannot be applied to {left_type} and {right_type}"
                    )))
                }
            }
            Type::Str => {
                let right_type = self.check_expr(right)?;
                if right_type == Type::Str {
                    Ok(Type::Str)
                } else {
                    Err(type_error(format!(
                        "operator 'Add' cannot be applied to {left_type} and {right_type}"
                    )))
                }
            }
            Type::Array(_) => {
                let right_type = self.check_expr_with_hint(right, Some(&left_type))?;
                if right_type == left_type {
                    Ok(left_type)
                } else {
                    Err(type_error(format!(
                        "operator 'Add' cannot be applied to {left_type} and {right_type}"
                    )))
                }
            }
            _ => {
                let right_type = self.check_expr(right)?;
                Err(type_error(format!(
                    "operator 'Add' cannot be applied to {left_type} and {right_type}"
                )))
            }
        }
    }

    fn check_call(&mut self, callee: &str, args: &[Expr]) -> FyrResult<Type> {
        match callee {
            "len" => {
                if args.len() != 1 {
                    return Err(type_error("len expects exactly one argument"));
                }

                match self.check_expr(&args[0])? {
                    Type::Array(_) | Type::Str => Ok(Type::I64),
                    found => Err(type_error(format!(
                        "len expects an array or str, found {found}"
                    ))),
                }
            }
            "range" => {
                if !(1..=2).contains(&args.len()) {
                    return Err(type_error("range expects one or two arguments"));
                }

                for (index, arg) in args.iter().enumerate() {
                    let found = self.check_expr(arg)?;
                    if found != Type::I64 {
                        return Err(type_error(format!(
                            "argument {} for range expected i64, found {found}",
                            index + 1
                        )));
                    }
                }

                Ok(Type::Array(Box::new(Type::I64)))
            }
            "assert" => {
                if !(1..=2).contains(&args.len()) {
                    return Err(type_error("assert expects one or two arguments"));
                }

                let condition = self.check_expr(&args[0])?;
                if condition != Type::Bool {
                    return Err(type_error(format!(
                        "assert condition expected bool, found {condition}"
                    )));
                }

                if let Some(message) = args.get(1) {
                    let found = self.check_expr(message)?;
                    if found != Type::Str {
                        return Err(type_error(format!(
                            "assert message expected str, found {found}"
                        )));
                    }
                }

                Ok(Type::Unit)
            }
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

        match merge_branch_types(then_type, else_type) {
            Some(ty) => Ok(ty),
            None => Err(type_error("if branches must have the same type")),
        }
    }

    fn check_if_statement(
        &mut self,
        condition: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
    ) -> FyrResult<Type> {
        if else_branch.is_empty() {
            let condition_type = self.check_expr(condition)?;
            if condition_type != Type::Bool {
                return Err(expected_type("bool", &condition_type));
            }
            self.check_block_scoped(then_branch)?;
            return Ok(Type::Unit);
        }

        self.check_if(condition, then_branch, else_branch)
    }

    fn check_return(&mut self, value: Option<&Expr>) -> FyrResult<Type> {
        let Some(expected) = self.return_types.last().cloned() else {
            return Err(type_error("return outside function"));
        };

        let found = match value {
            Some(value) => self.check_expr(value)?,
            None => Type::Unit,
        };

        if found != expected && found != Type::Never {
            return Err(type_error(format!(
                "return expected {expected}, found {found}"
            )));
        }

        Ok(Type::Never)
    }

    fn check_while(&mut self, condition: &Expr, body: &[Statement]) -> FyrResult<()> {
        let condition_type = self.check_expr(condition)?;
        if condition_type != Type::Bool {
            return Err(expected_type("bool", &condition_type));
        }

        self.loop_depth += 1;
        let result = self.check_block_scoped(body);
        self.loop_depth -= 1;
        result?;
        Ok(())
    }

    fn check_for(&mut self, name: &str, iterable: &Expr, body: &[Statement]) -> FyrResult<()> {
        let iterable_type = self.check_expr(iterable)?;
        let Type::Array(element_type) = iterable_type else {
            return Err(type_error(format!(
                "for loop expected an array, found {iterable_type}"
            )));
        };

        self.loop_depth += 1;
        self.push_scope();
        self.define(name, *element_type, false);
        let result = self.check_block(body);
        self.pop_scope();
        self.loop_depth -= 1;
        result?;
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
            if last_type == Type::Never {
                return Ok(Type::Never);
            }
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
            TypeName::Struct(name) => Type::Struct(name.clone()),
            TypeName::Array(element) => Type::Array(Box::new(element.as_type())),
        }
    }
}

impl Checker {
    fn validate_type_name(&self, ty: &TypeName) -> FyrResult<()> {
        match ty {
            TypeName::Infer | TypeName::I64 | TypeName::Bool | TypeName::Str | TypeName::Unit => {
                Ok(())
            }
            TypeName::Struct(name) if self.structs.contains_key(name) => Ok(()),
            TypeName::Struct(name) => Err(type_error(format!("unknown type '{name}'"))),
            TypeName::Array(element) => self.validate_type_name(element),
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

fn merge_branch_types(left: Type, right: Type) -> Option<Type> {
    match (left, right) {
        (Type::Never, Type::Never) => Some(Type::Never),
        (Type::Never, ty) | (ty, Type::Never) => Some(ty),
        (left, right) if left == right => Some(left),
        _ => None,
    }
}

fn is_equatable_type(ty: &Type) -> bool {
    match ty {
        Type::I64 | Type::Bool | Type::Str | Type::Unit | Type::Struct(_) => true,
        Type::Array(element) => is_equatable_type(element),
        Type::Infer | Type::Never | Type::Function { .. } => false,
    }
}

fn is_empty_array_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Array(elements) if elements.is_empty())
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
    fn accepts_return_break_and_continue() {
        typecheck(
            r#"
fn first(limit: i64) -> i64:
    var i = 0
    while i < limit:
        i = i + 1
        if i == 2:
            continue
        else:
            i = i
        if i == 4:
            break
        else:
            i = i
        if i % 3 == 0:
            return i
        else:
            i = i
    return -1
"#,
        )
        .expect("control-flow exits should typecheck");
    }

    #[test]
    fn rejects_return_outside_function() {
        let error = typecheck("return 1\n").expect_err("top-level return should fail");

        assert!(error.message.contains("return outside function"));
    }

    #[test]
    fn rejects_wrong_return_type() {
        let error = typecheck(
            r#"
fn bad() -> i64:
    return "no"
"#,
        )
        .expect_err("wrong return type should fail");

        assert!(error.message.contains("return expected i64"));
    }

    #[test]
    fn rejects_break_and_continue_outside_loop() {
        let break_error = typecheck("break\n").expect_err("top-level break should fail");
        let continue_error = typecheck("continue\n").expect_err("top-level continue should fail");

        assert!(break_error.message.contains("break outside loop"));
        assert!(continue_error.message.contains("continue outside loop"));
    }

    #[test]
    fn accepts_struct_construction_and_field_access() {
        typecheck(
            r#"
struct Point:
    x: i64
    y: i64

fn length_squared(p: Point) -> i64:
    return p.x * p.x + p.y * p.y

let p = Point { x: 3, y: 4 }
length_squared(p)
"#,
        )
        .expect("struct program should typecheck");
    }

    #[test]
    fn rejects_struct_field_type_mismatch() {
        let error = typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: "four" }
"#,
        )
        .expect_err("wrong field type should fail");

        assert!(error.message.contains("field 'y' expected i64"));
    }

    #[test]
    fn rejects_missing_struct_fields() {
        let error = typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let p = Point { x: 3 }
"#,
        )
        .expect_err("missing field should fail");

        assert!(error.message.contains("missing field 'y'"));
    }

    #[test]
    fn rejects_unknown_field_access() {
        let error = typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
p.z
"#,
        )
        .expect_err("unknown field should fail");

        assert!(error.message.contains("no field 'z'"));
    }

    #[test]
    fn accepts_arrays_and_indexing() {
        typecheck(
            r#"
fn sum(values: [i64]) -> i64:
    var total = 0
    var i = 0
    while i < len(values):
        total = total + values[i]
        i = i + 1
    return total

let values = [1, 2, 3, 4]
sum(values)
"#,
        )
        .expect("array program should typecheck");
    }

    #[test]
    fn accepts_annotated_bindings_and_empty_arrays() {
        typecheck(
            r#"
let limit: i64 = 4
var values: [i64] = []
len(values) + limit
"#,
        )
        .expect("annotated bindings should typecheck");
    }

    #[test]
    fn rejects_binding_annotation_mismatch() {
        let error = typecheck("let answer: bool = 42\n")
            .expect_err("binding annotation mismatch should fail");

        assert!(error.message.contains("binding 'answer' expected bool"));
    }

    #[test]
    fn accepts_for_loop_over_arrays() {
        typecheck(
            r#"
fn sum(values: [i64]) -> i64:
    var total = 0
    for value in values:
        total = total + value
    return total

sum([1, 2, 3])
"#,
        )
        .expect("for loop should typecheck");
    }

    #[test]
    fn accepts_if_statement_without_else() {
        typecheck(
            r#"
var total = 0
if true:
    total = 42

total
"#,
        )
        .expect("if statement should typecheck");
    }

    #[test]
    fn rejects_for_loop_over_non_array() {
        let error = typecheck(
            r#"
for value in 42:
    print(value)
"#,
        )
        .expect_err("for over non-array should fail");

        assert!(error.message.contains("for loop expected an array"));
    }

    #[test]
    fn rejects_assignment_to_for_binding() {
        let error = typecheck(
            r#"
for value in [1, 2]:
    value = 3
"#,
        )
        .expect_err("for binding assignment should fail");

        assert!(error.message.contains("immutable binding 'value'"));
    }

    #[test]
    fn accepts_range_as_i64_array() {
        typecheck(
            r#"
var total = 0
for value in range(1, 5):
    total = total + value

total
"#,
        )
        .expect("range should typecheck as [i64]");
    }

    #[test]
    fn rejects_non_integer_range_arguments() {
        let error = typecheck("range(true)\n").expect_err("range type error should fail");

        assert!(error.message.contains("argument 1 for range expected i64"));
    }

    #[test]
    fn rejects_wrong_range_arity() {
        let error = typecheck("range(1, 2, 3)\n").expect_err("range arity should fail");

        assert!(error.message.contains("range expects one or two arguments"));
    }

    #[test]
    fn accepts_assertions() {
        typecheck("assert(true)\nassert(1 < 2, \"ordered\")\n")
            .expect("assertions should typecheck");
    }

    #[test]
    fn rejects_non_bool_assert_condition() {
        let error = typecheck("assert(1)\n").expect_err("assert condition should fail");

        assert!(error.message.contains("assert condition expected bool"));
    }

    #[test]
    fn rejects_non_string_assert_message() {
        let error = typecheck("assert(true, 1)\n").expect_err("assert message should fail");

        assert!(error.message.contains("assert message expected str"));
    }

    #[test]
    fn accepts_data_equality() {
        typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let a = Point { x: 3, y: 4 }
let b = Point { x: 3, y: 4 }

assert([1, 2] == [1, 2])
assert(a == b)
"#,
        )
        .expect("data equality should typecheck");
    }

    #[test]
    fn rejects_function_equality() {
        let error = typecheck(
            r#"
fn id(value: i64) -> i64:
    value

id == id
"#,
        )
        .expect_err("function equality should fail");

        assert!(error.message.contains("cannot be compared for equality"));
    }

    #[test]
    fn accepts_array_concatenation() {
        typecheck(
            r#"
let left = [1, 2]
let right = [3, 4]
let combined = left + right
assert(combined == [1, 2, 3, 4])
"#,
        )
        .expect("array concatenation should typecheck");
    }

    #[test]
    fn infers_empty_array_concatenation_from_other_side() {
        typecheck(
            r#"
let left = [1, 2] + []
let right = [] + [3, 4]
assert(left == [1, 2])
assert(right == [3, 4])
"#,
        )
        .expect("empty concat side should use other array type");
    }

    #[test]
    fn rejects_array_concatenation_type_mismatch() {
        let error = typecheck("let mixed = [1] + [true]\n")
            .expect_err("mixed array concatenation should fail");

        assert!(error.message.contains("array element expected i64"));
    }

    #[test]
    fn rejects_mixed_array_elements() {
        let error = typecheck("let values = [1, true]\n").expect_err("mixed elements should fail");

        assert!(error.message.contains("array element expected i64"));
    }

    #[test]
    fn rejects_non_integer_array_index() {
        let error = typecheck("let values = [1, 2]\nvalues[true]\n")
            .expect_err("non-integer index should fail");

        assert!(error.message.contains("array index expected i64"));
    }

    #[test]
    fn rejects_untyped_empty_array_literals_for_now() {
        let error = typecheck("let values = []\n").expect_err("untyped empty array should fail");

        assert!(error.message.contains("empty array literals"));
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

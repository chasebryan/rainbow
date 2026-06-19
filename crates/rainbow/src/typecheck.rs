use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

use crate::ast::{
    BinaryOp, EnumVariant, Expr, IfLetPattern, MatchArm, MatchPattern, Param, Program, Statement,
    TypeName, UnaryOp,
};
use crate::diagnostic::{RainbowError, RainbowResult};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    Infer,
    Nil,
    Never,
    I64,
    F64,
    Bool,
    Str,
    Unit,
    Struct(String),
    Array(Box<Type>),
    Nullable(Box<Type>),
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
}

impl Display for Type {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Infer => write!(f, "infer"),
            Type::Nil => write!(f, "nil"),
            Type::Never => write!(f, "never"),
            Type::I64 => write!(f, "i64"),
            Type::F64 => write!(f, "f64"),
            Type::Bool => write!(f, "bool"),
            Type::Str => write!(f, "str"),
            Type::Unit => write!(f, "unit"),
            Type::Struct(name) => write!(f, "{name}"),
            Type::Array(element) => write!(f, "[{element}]"),
            Type::Nullable(inner) => write!(f, "{inner}?"),
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

pub fn check(program: &Program) -> RainbowResult<()> {
    Checker::new().check_program(program)
}

struct Checker {
    scopes: Vec<HashMap<String, Binding>>,
    structs: HashMap<String, Vec<Param>>,
    enums: HashMap<String, Vec<EnumVariant>>,
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
            enums: HashMap::new(),
            return_types: Vec::new(),
            loop_depth: 0,
        }
    }

    fn check_program(mut self, program: &Program) -> RainbowResult<()> {
        self.predeclare_enums(&program.statements)?;
        self.predeclare_structs(&program.statements)?;
        self.validate_enum_payloads(&program.statements)?;
        self.predeclare_functions(&program.statements)?;

        for statement in &program.statements {
            self.check_statement(statement)?;
        }

        Ok(())
    }

    fn predeclare_enums(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            if let Statement::Enum { name, variants, .. } = statement {
                let span = statement.span();
                let source_path = statement.source_path();
                if self.enums.contains_key(name)
                    || self.structs.contains_key(name)
                    || self.current_scope().contains_key(name)
                {
                    return Err(type_error(format!("enum '{name}' already exists"))
                        .with_fallback_location(span, source_path));
                }
                reject_duplicate_variants("enum", name, "variant", variants)
                    .map_err(|error| error.with_fallback_location(span, source_path))?;

                self.enums.insert(name.clone(), variants.clone());
            }
        }

        Ok(())
    }

    fn predeclare_structs(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            if let Statement::Struct { name, fields, .. } = statement {
                let span = statement.span();
                let source_path = statement.source_path();
                if self.structs.contains_key(name)
                    || self.enums.contains_key(name)
                    || self.current_scope().contains_key(name)
                {
                    return Err(type_error(format!("struct '{name}' already exists"))
                        .with_fallback_location(span, source_path));
                }
                reject_duplicate_members("struct", name, "field", fields)
                    .map_err(|error| error.with_fallback_location(span, source_path))?;

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

    fn validate_enum_payloads(&self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            let Statement::Enum { name, variants, .. } = statement else {
                continue;
            };
            let source_path = statement.source_path();
            for variant in variants {
                if let Some(payload) = &variant.payload {
                    if payload == &TypeName::Infer {
                        return Err(type_error(format!(
                            "enum '{name}' variant '{}' needs an explicit payload type",
                            variant.name
                        ))
                        .with_fallback_location(variant.span, source_path));
                    }
                    self.validate_type_name(payload)
                        .map_err(|error| error.with_fallback_location(variant.span, source_path))?;
                }
            }
        }

        Ok(())
    }

    fn predeclare_functions(&mut self, statements: &[Statement]) -> RainbowResult<()> {
        for statement in statements {
            if let Statement::Fn {
                name,
                params,
                return_type,
                ..
            } = statement
            {
                let span = statement.span();
                let source_path = statement.source_path();
                let signature = self
                    .function_signature(name, params, return_type)
                    .map_err(|error| error.with_fallback_location(span, source_path))?;
                self.define(name, signature, false)
                    .map_err(|error| error.with_fallback_location(span, source_path))?;
            }
        }

        Ok(())
    }

    fn check_statement(&mut self, statement: &Statement) -> RainbowResult<Type> {
        let span = statement.span();
        let source_path = statement.source_path();
        let result = match statement {
            Statement::Struct { .. } => Ok(Type::Unit),
            Statement::Enum { .. } => Ok(Type::Unit),
            Statement::Import { .. } => Ok(Type::Unit),
            Statement::Let {
                name, ty, value, ..
            } => self.check_binding(name, ty, value, false),
            Statement::Var {
                name, ty, value, ..
            } => self.check_binding(name, ty, value, true),
            Statement::Assign { name, value, .. } => {
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

                if !type_compatible(&binding.ty, &value_type) {
                    return Err(type_error(format!(
                        "assignment to '{name}' expected {}, found {value_type}",
                        binding.ty
                    )));
                }

                Ok(Type::Unit)
            }
            Statement::Fn {
                name,
                params,
                return_type,
                body,
                ..
            } => self.check_function_statement(name, params, return_type, body),
            Statement::While {
                condition, body, ..
            } => {
                self.check_while(condition, body)?;
                Ok(Type::Unit)
            }
            Statement::For {
                name,
                iterable,
                body,
                ..
            } => {
                self.check_for(name, iterable, body)?;
                Ok(Type::Unit)
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => self.check_if_statement(condition, then_branch, else_branch),
            Statement::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
                ..
            } => self.check_if_let_statement(pattern, value, then_branch, else_branch),
            Statement::Return { value, .. } => self.check_return(value.as_ref()),
            Statement::Break { .. } => {
                if self.loop_depth == 0 {
                    return Err(type_error("break outside loop"));
                }
                Ok(Type::Never)
            }
            Statement::Continue { .. } => {
                if self.loop_depth == 0 {
                    return Err(type_error("continue outside loop"));
                }
                Ok(Type::Never)
            }
            Statement::Expr { expr, .. } => self.check_expr(expr),
        };

        result.map_err(|error| error.with_fallback_location(span, source_path))
    }

    fn check_binding(
        &mut self,
        name: &str,
        annotation: &TypeName,
        value: &Expr,
        mutable: bool,
    ) -> RainbowResult<Type> {
        let expected = if *annotation == TypeName::Infer {
            None
        } else {
            self.validate_type_name(annotation)?;
            Some(annotation.as_type())
        };

        let value_type = self.check_expr_with_hint(value, expected.as_ref())?;
        let binding_type = if let Some(expected) = expected {
            if !type_compatible(&expected, &value_type) {
                return Err(type_error(format!(
                    "binding '{name}' expected {expected}, found {value_type}"
                )));
            }
            expected
        } else {
            if value_type == Type::Nil {
                return Err(type_error(format!(
                    "binding '{name}' needs an explicit nullable type for nil"
                )));
            }
            value_type
        };

        self.define(name, binding_type, mutable)?;
        Ok(Type::Unit)
    }

    fn check_function_statement(
        &mut self,
        name: &str,
        params: &[Param],
        return_type: &TypeName,
        body: &[Statement],
    ) -> RainbowResult<Type> {
        let signature = self.function_signature(name, params, return_type)?;
        if !self.is_predeclared_top_level_function(name, &signature) {
            self.define(name, signature, false)?;
        }

        let expected = return_type.as_type();
        self.push_scope();
        self.return_types.push(expected.clone());
        for Param { name, ty } in params {
            self.define(name, ty.as_type(), false)?;
        }
        let body_type = self.check_block(body)?;
        self.return_types.pop();
        self.pop_scope();

        if !type_compatible(&expected, &body_type) && body_type != Type::Never {
            return Err(type_error(format!(
                "function returns {body_type}, but signature says {expected}"
            )));
        }

        Ok(Type::Unit)
    }

    fn function_signature(
        &self,
        name: &str,
        params: &[Param],
        return_type: &TypeName,
    ) -> RainbowResult<Type> {
        reject_inferred_signature(name, params, return_type)?;
        reject_duplicate_members("function", name, "parameter", params)?;
        for param in params {
            self.validate_type_name(&param.ty)?;
        }
        self.validate_type_name(return_type)?;

        Ok(Type::Function {
            params: params.iter().map(|param| param.ty.as_type()).collect(),
            return_type: Box::new(return_type.as_type()),
        })
    }

    fn is_predeclared_top_level_function(&self, name: &str, signature: &Type) -> bool {
        self.scopes.len() == 1
            && self
                .scopes
                .last()
                .and_then(|scope| scope.get(name))
                .is_some_and(|binding| &binding.ty == signature)
    }

    fn check_expr(&mut self, expr: &Expr) -> RainbowResult<Type> {
        self.check_expr_with_hint(expr, None)
    }

    fn check_expr_with_hint(
        &mut self,
        expr: &Expr,
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        match expr {
            Expr::Int(_) => Ok(Type::I64),
            Expr::Float(_) => Ok(Type::F64),
            Expr::Bool(_) => Ok(Type::Bool),
            Expr::Str(_) => Ok(Type::Str),
            Expr::Nil => match expected {
                Some(expected @ Type::Nullable(_)) => Ok(expected.clone()),
                _ => Ok(Type::Nil),
            },
            Expr::Variable(name) => self
                .lookup(name)
                .map(|binding| binding.ty.clone())
                .ok_or_else(|| type_error(format!("unknown binding '{name}'"))),
            Expr::Unary { op, expr } => {
                let expr_type = self.check_expr(expr)?;
                match (op, expr_type) {
                    (UnaryOp::Negate, Type::I64) => Ok(Type::I64),
                    (UnaryOp::Negate, Type::F64) => Ok(Type::F64),
                    (UnaryOp::Not, Type::Bool) => Ok(Type::Bool),
                    (UnaryOp::Negate, found) => Err(expected_type("i64 or f64", &found)),
                    (UnaryOp::Not, found) => Err(expected_type("bool", &found)),
                }
            }
            Expr::Binary { left, op, right } => self.check_binary(left, *op, right, expected),
            Expr::Call { callee, args } => self.check_call(callee, args, expected),
            Expr::Flow {
                value,
                callee,
                args,
            } => self.check_flow(value, callee, args, expected),
            Expr::StructInit { name, fields } => self.check_struct_init(name, fields),
            Expr::EnumInit {
                enum_name,
                variant,
                value,
            } => self.check_enum_init(enum_name, variant, value.as_deref()),
            Expr::Field { object, field } => self.check_field(object, field),
            Expr::Array(elements) => self.check_array(elements, expected),
            Expr::Index { collection, index } => self.check_index(collection, index),
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => self.check_if(condition, then_branch, else_branch, expected),
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => self.check_if_let(pattern, value, then_branch, else_branch, expected),
            Expr::Match { value, arms } => self.check_match(value, arms, expected),
        }
    }

    fn check_array(&mut self, elements: &[Expr], expected: Option<&Type>) -> RainbowResult<Type> {
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

        if let Some(expected_element) = expected_element {
            for element in elements {
                let found = self.check_expr_with_hint(element, Some(expected_element))?;
                if !type_compatible(expected_element, &found) {
                    return Err(type_error(format!(
                        "array element expected {expected_element}, found {found}"
                    )));
                }
            }

            return Ok(Type::Array(Box::new(expected_element.clone())));
        }

        let element_type = self.check_expr_with_hint(first, None)?;
        if element_type == Type::Nil {
            return Err(type_error(
                "nil array elements need an expected nullable type",
            ));
        }
        for element in elements.iter().skip(1) {
            let found = self.check_expr_with_hint(element, None)?;
            if found != element_type {
                return Err(type_error(format!(
                    "array element expected {element_type}, found {found}"
                )));
            }
        }

        Ok(Type::Array(Box::new(element_type)))
    }

    fn check_index(&mut self, collection: &Expr, index: &Expr) -> RainbowResult<Type> {
        let collection_type = self.check_expr(collection)?;
        match collection_type {
            Type::Array(element_type) => {
                let index_type = self.check_expr(index)?;
                if index_type != Type::I64 {
                    return Err(type_error(format!(
                        "array index expected i64, found {index_type}"
                    )));
                }
                Ok(*element_type)
            }
            Type::Str => {
                let index_type = self.check_expr(index)?;
                if index_type != Type::I64 {
                    return Err(type_error(format!(
                        "string index expected i64, found {index_type}"
                    )));
                }
                Ok(Type::Str)
            }
            found => Err(type_error(format!(
                "indexing expected an array or str, found {found}"
            ))),
        }
    }

    fn check_struct_init(&mut self, name: &str, fields: &[(String, Expr)]) -> RainbowResult<Type> {
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
            let found = self.check_expr_with_hint(value, Some(&expected))?;
            if !type_compatible(&expected, &found) {
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

    fn check_field(&mut self, object: &Expr, field_name: &str) -> RainbowResult<Type> {
        if let Expr::Variable(enum_name) = object
            && let Some(variants) = self.enums.get(enum_name)
        {
            if let Some(variant) = variants.iter().find(|variant| variant.name == field_name) {
                if variant.payload.is_some() {
                    return Err(type_error(format!(
                        "enum variant '{enum_name}.{field_name}' needs a payload"
                    )));
                }
                return Ok(Type::Struct(enum_name.clone()));
            }

            return Err(type_error(format!(
                "enum '{enum_name}' has no variant '{field_name}'"
            )));
        }

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

    fn check_enum_init(
        &mut self,
        enum_name: &str,
        variant_name: &str,
        value: Option<&Expr>,
    ) -> RainbowResult<Type> {
        let variant = self
            .enums
            .get(enum_name)
            .and_then(|variants| variants.iter().find(|variant| variant.name == variant_name))
            .cloned()
            .ok_or_else(|| {
                if self.enums.contains_key(enum_name) {
                    type_error(format!(
                        "enum '{enum_name}' has no variant '{variant_name}'"
                    ))
                } else {
                    type_error(format!("unknown enum '{enum_name}'"))
                }
            })?;

        match (&variant.payload, value) {
            (Some(expected), Some(value)) => {
                let expected = expected.as_type();
                let found = self.check_expr_with_hint(value, Some(&expected))?;
                if !type_compatible(&expected, &found) {
                    return Err(type_error(format!(
                        "enum variant '{enum_name}.{variant_name}' expected {expected}, found {found}"
                    )));
                }
            }
            (Some(expected), None) => {
                return Err(type_error(format!(
                    "enum variant '{enum_name}.{variant_name}' expected payload {}",
                    expected.as_type()
                )));
            }
            (None, Some(_)) => {
                return Err(type_error(format!(
                    "enum variant '{enum_name}.{variant_name}' does not take a payload"
                )));
            }
            (None, None) => {
                return Err(type_error(format!(
                    "enum variant '{enum_name}.{variant_name}' is a value and should not be called"
                )));
            }
        }

        Ok(Type::Struct(enum_name.to_owned()))
    }

    fn check_binary(
        &mut self,
        left: &Expr,
        op: BinaryOp,
        right: &Expr,
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        if op == BinaryOp::Coalesce {
            return self.check_coalesce(left, right, expected);
        }

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
            BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder
                if left_type == Type::F64 && right_type == Type::F64 =>
            {
                Ok(Type::F64)
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
                if left_type == Type::I64 && right_type == Type::I64 =>
            {
                Ok(Type::Bool)
            }
            BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual
                if left_type == Type::F64 && right_type == Type::F64 =>
            {
                Ok(Type::Bool)
            }
            BinaryOp::Equal | BinaryOp::NotEqual
                if equality_compatible(&left_type, &right_type) =>
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

    fn check_coalesce(
        &mut self,
        left: &Expr,
        right: &Expr,
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        let left_type = self.check_expr(left)?;

        match left_type {
            Type::Nullable(inner) => {
                let nullable_type = Type::Nullable(inner.clone());
                let found = self.check_expr_with_hint(right, Some(&inner))?;
                if type_compatible(&inner, &found) {
                    return Ok(*inner);
                }

                if type_compatible(&nullable_type, &found) {
                    return Ok(nullable_type);
                }

                Err(type_error(format!(
                    "coalesce fallback expected {inner} or {nullable_type}, found {found}"
                )))
            }
            Type::Nil => {
                let found = self.check_expr_with_hint(right, expected)?;
                if found == Type::Nil {
                    return Err(type_error("coalesce fallback needs a concrete type"));
                }
                Ok(found)
            }
            found => Err(type_error(format!(
                "coalesce left operand expected nullable, found {found}"
            ))),
        }
    }

    fn check_add(&mut self, left: &Expr, right: &Expr) -> RainbowResult<Type> {
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
            Type::F64 => {
                let right_type = self.check_expr(right)?;
                if right_type == Type::F64 {
                    Ok(Type::F64)
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
                if type_compatible(&left_type, &right_type) {
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

    fn check_call(
        &mut self,
        callee: &str,
        args: &[Expr],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        match callee {
            "i64" => self.check_numeric_conversion(callee, args, Type::I64),
            "f64" => self.check_numeric_conversion(callee, args, Type::F64),
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
            "contains" => {
                if args.len() != 2 {
                    return Err(type_error("contains expects exactly two arguments"));
                }

                let collection_type = self.check_expr(&args[0])?;
                match collection_type {
                    Type::Array(element) => {
                        if !is_equatable_type(&element) {
                            return Err(type_error(format!(
                                "{element} cannot be checked with contains"
                            )));
                        }

                        let found = self.check_expr_with_hint(&args[1], Some(&element))?;
                        if !type_compatible(&element, &found) {
                            return Err(type_error(format!(
                                "contains expected {element}, found {found}"
                            )));
                        }

                        Ok(Type::Bool)
                    }
                    Type::Str => {
                        let found = self.check_expr(&args[1])?;
                        if found != Type::Str {
                            return Err(type_error(format!(
                                "contains(str, value) expected str, found {found}"
                            )));
                        }

                        Ok(Type::Bool)
                    }
                    found => Err(type_error(format!(
                        "contains expects an array or str, found {found}"
                    ))),
                }
            }
            "find" => {
                if args.len() != 2 {
                    return Err(type_error("find expects exactly two arguments"));
                }

                if is_empty_array_literal(&args[0]) {
                    let found = self.check_expr(&args[1])?;
                    if !is_equatable_type(&found) {
                        return Err(type_error(format!("{found} cannot be searched with find")));
                    }
                    return Ok(Type::I64);
                }

                let collection_type = self.check_expr(&args[0])?;
                match collection_type {
                    Type::Array(element) => {
                        if !is_equatable_type(&element) {
                            return Err(type_error(format!(
                                "{element} cannot be searched with find"
                            )));
                        }

                        let found = self.check_expr_with_hint(&args[1], Some(&element))?;
                        if !type_compatible(&element, &found) {
                            return Err(type_error(format!(
                                "find expected {element}, found {found}"
                            )));
                        }

                        Ok(Type::I64)
                    }
                    Type::Str => {
                        let found = self.check_expr(&args[1])?;
                        if found != Type::Str {
                            return Err(type_error(format!(
                                "find(str, value) expected str, found {found}"
                            )));
                        }

                        Ok(Type::I64)
                    }
                    found => Err(type_error(format!(
                        "find expects an array or str, found {found}"
                    ))),
                }
            }
            "count" => {
                if args.len() != 2 {
                    return Err(type_error("count expects exactly two arguments"));
                }

                if is_empty_array_literal(&args[0]) {
                    let found = self.check_expr(&args[1])?;
                    if !is_equatable_type(&found) {
                        return Err(type_error(format!("{found} cannot be counted with count")));
                    }
                    return Ok(Type::I64);
                }

                let collection_type = self.check_expr(&args[0])?;
                match collection_type {
                    Type::Array(element) => {
                        if !is_equatable_type(&element) {
                            return Err(type_error(format!(
                                "{element} cannot be counted with count"
                            )));
                        }

                        let found = self.check_expr_with_hint(&args[1], Some(&element))?;
                        if !type_compatible(&element, &found) {
                            return Err(type_error(format!(
                                "count expected {element}, found {found}"
                            )));
                        }

                        Ok(Type::I64)
                    }
                    Type::Str => {
                        let found = self.check_expr(&args[1])?;
                        if found != Type::Str {
                            return Err(type_error(format!(
                                "count(str, value) expected str, found {found}"
                            )));
                        }

                        Ok(Type::I64)
                    }
                    found => Err(type_error(format!(
                        "count expects an array or str, found {found}"
                    ))),
                }
            }
            "is_empty" => {
                if args.len() != 1 {
                    return Err(type_error("is_empty expects exactly one argument"));
                }

                if is_empty_array_literal(&args[0]) {
                    return Ok(Type::Bool);
                }

                match self.check_expr(&args[0])? {
                    Type::Array(_) | Type::Str => Ok(Type::Bool),
                    found => Err(type_error(format!(
                        "is_empty expects an array or str, found {found}"
                    ))),
                }
            }
            "get" => {
                if args.len() != 3 {
                    return Err(type_error("get expects exactly three arguments"));
                }

                let index_type = self.check_expr(&args[1])?;
                if index_type != Type::I64 {
                    return Err(type_error(format!(
                        "get index expected i64, found {index_type}"
                    )));
                }

                if is_empty_array_literal(&args[0]) {
                    return self.check_expr_with_hint(&args[2], expected);
                }

                match self.check_expr(&args[0])? {
                    Type::Array(element) => {
                        let found = self.check_expr_with_hint(&args[2], Some(&element))?;
                        if !type_compatible(&element, &found) {
                            return Err(type_error(format!(
                                "get default expected {element}, found {found}"
                            )));
                        }
                        Ok(*element)
                    }
                    Type::Str => {
                        let found = self.check_expr(&args[2])?;
                        if found != Type::Str {
                            return Err(type_error(format!(
                                "get default expected str, found {found}"
                            )));
                        }
                        Ok(Type::Str)
                    }
                    found => Err(type_error(format!(
                        "get expects an array or str, found {found}"
                    ))),
                }
            }
            "append" => {
                if args.len() != 2 {
                    return Err(type_error("append expects exactly two arguments"));
                }

                if is_empty_array_literal(&args[0]) {
                    let expected_element = match expected {
                        Some(Type::Array(element)) => Some(element.as_ref()),
                        _ => None,
                    };
                    let found = self.check_expr_with_hint(&args[1], expected_element)?;
                    if let Some(expected_element) = expected_element
                        && !type_compatible(expected_element, &found)
                    {
                        return Err(type_error(format!(
                            "append expected {expected_element}, found {found}"
                        )));
                    }
                    return Ok(Type::Array(Box::new(found)));
                }

                let collection_type = self.check_expr(&args[0])?;
                let Type::Array(element) = collection_type else {
                    return Err(type_error(format!(
                        "append expects an array, found {collection_type}"
                    )));
                };

                let found = self.check_expr_with_hint(&args[1], Some(&element))?;
                if !type_compatible(&element, &found) {
                    return Err(type_error(format!(
                        "append expected {element}, found {found}"
                    )));
                }

                Ok(Type::Array(element))
            }
            "reverse" => {
                if args.len() != 1 {
                    return Err(type_error("reverse expects exactly one argument"));
                }

                if is_empty_array_literal(&args[0]) {
                    let Some(Type::Array(element)) = expected else {
                        return Err(type_error(
                            "reverse needs an expected array type for empty array literals",
                        ));
                    };
                    let expected_array = Type::Array(element.clone());
                    self.check_expr_with_hint(&args[0], Some(&expected_array))?;
                    return Ok(expected_array);
                }

                let hint = match expected {
                    Some(Type::Array(_)) | Some(Type::Str) => expected,
                    _ => None,
                };
                let collection_type = self.check_expr_with_hint(&args[0], hint)?;
                match collection_type {
                    Type::Array(_) | Type::Str => Ok(collection_type),
                    found => Err(type_error(format!(
                        "reverse expects an array or str, found {found}"
                    ))),
                }
            }
            "first" => self.check_edge_call("first", args, expected),
            "last" => self.check_edge_call("last", args, expected),
            "trim" | "lower" | "upper" => self.check_string_unary(callee, args),
            "starts_with" | "ends_with" => self.check_string_predicate(callee, args),
            "replace" => self.check_replace(args),
            "split" => self.check_split(args),
            "join" => self.check_join(args),
            "slice" => {
                if args.len() != 3 {
                    return Err(type_error("slice expects exactly three arguments"));
                }

                let collection_type = self.check_expr(&args[0])?;
                for (label, arg) in [("start", &args[1]), ("end", &args[2])] {
                    let found = self.check_expr(arg)?;
                    if found != Type::I64 {
                        return Err(type_error(format!(
                            "slice {label} expected i64, found {found}"
                        )));
                    }
                }

                match collection_type {
                    Type::Array(element) => Ok(Type::Array(element)),
                    Type::Str => Ok(Type::Str),
                    found => Err(type_error(format!(
                        "slice expects an array or str, found {found}"
                    ))),
                }
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
                    let found = self.check_expr_with_hint(arg, Some(expected))?;
                    if !type_compatible(expected, &found) {
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

    fn check_flow(
        &mut self,
        value: &Expr,
        callee: &str,
        args: &[Expr],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        let mut flowd_args = Vec::with_capacity(args.len() + 1);
        flowd_args.push(value.clone());
        flowd_args.extend(args.iter().cloned());
        self.check_call(callee, &flowd_args, expected)
    }

    fn check_numeric_conversion(
        &mut self,
        name: &str,
        args: &[Expr],
        result: Type,
    ) -> RainbowResult<Type> {
        if args.len() != 1 {
            return Err(type_error(format!("{name} expects exactly one argument")));
        }

        match self.check_expr(&args[0])? {
            Type::I64 | Type::F64 => Ok(result),
            found => Err(type_error(format!(
                "{name} conversion expects i64 or f64, found {found}"
            ))),
        }
    }

    fn check_string_unary(&mut self, name: &str, args: &[Expr]) -> RainbowResult<Type> {
        if args.len() != 1 {
            return Err(type_error(format!("{name} expects exactly one argument")));
        }

        self.check_string_arg(&args[0], name)?;
        Ok(Type::Str)
    }

    fn check_string_predicate(&mut self, name: &str, args: &[Expr]) -> RainbowResult<Type> {
        if args.len() != 2 {
            return Err(type_error(format!("{name} expects exactly two arguments")));
        }

        self.check_string_arg(&args[0], name)?;
        self.check_string_arg(&args[1], &format!("{name} value"))?;
        Ok(Type::Bool)
    }

    fn check_replace(&mut self, args: &[Expr]) -> RainbowResult<Type> {
        if args.len() != 3 {
            return Err(type_error("replace expects exactly three arguments"));
        }

        self.check_string_arg(&args[0], "replace")?;
        self.check_string_arg(&args[1], "replace old")?;
        self.check_string_arg(&args[2], "replace new")?;
        Ok(Type::Str)
    }

    fn check_split(&mut self, args: &[Expr]) -> RainbowResult<Type> {
        if args.len() != 2 {
            return Err(type_error("split expects exactly two arguments"));
        }

        self.check_string_arg(&args[0], "split")?;
        self.check_string_arg(&args[1], "split separator")?;
        Ok(Type::Array(Box::new(Type::Str)))
    }

    fn check_join(&mut self, args: &[Expr]) -> RainbowResult<Type> {
        if args.len() != 2 {
            return Err(type_error("join expects exactly two arguments"));
        }

        if !is_empty_array_literal(&args[0]) {
            match self.check_expr(&args[0])? {
                Type::Array(element) if *element == Type::Str => {}
                found => return Err(type_error(format!("join expects [str], found {found}"))),
            }
        }

        self.check_string_arg(&args[1], "join separator")?;
        Ok(Type::Str)
    }

    fn check_string_arg(&mut self, arg: &Expr, context: &str) -> RainbowResult<()> {
        let found = self.check_expr(arg)?;
        if found != Type::Str {
            return Err(type_error(format!("{context} expected str, found {found}")));
        }
        Ok(())
    }

    fn check_edge_call(
        &mut self,
        name: &str,
        args: &[Expr],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        if args.len() != 2 {
            return Err(type_error(format!("{name} expects exactly two arguments")));
        }

        if is_empty_array_literal(&args[0]) {
            return self.check_expr_with_hint(&args[1], expected);
        }

        match self.check_expr(&args[0])? {
            Type::Array(element) => {
                let found = self.check_expr_with_hint(&args[1], Some(&element))?;
                if !type_compatible(&element, &found) {
                    return Err(type_error(format!(
                        "{name} default expected {element}, found {found}"
                    )));
                }
                Ok(*element)
            }
            Type::Str => {
                let found = self.check_expr(&args[1])?;
                if found != Type::Str {
                    return Err(type_error(format!(
                        "{name} default expected str, found {found}"
                    )));
                }
                Ok(Type::Str)
            }
            found => Err(type_error(format!(
                "{name} expects an array or str, found {found}"
            ))),
        }
    }

    fn check_if(
        &mut self,
        condition: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        let condition_type = self.check_expr(condition)?;
        if condition_type != Type::Bool {
            return Err(expected_type("bool", &condition_type));
        }

        let (then_type, else_type) = if expected.is_none()
            && block_ends_with_array_hint_hole(then_branch)
            && !block_ends_with_array_hint_hole(else_branch)
        {
            let else_type = self.check_block_scoped(else_branch)?;
            let then_type =
                self.check_block_scoped_with_hint(then_branch, array_type_hint(&else_type))?;
            (then_type, else_type)
        } else if expected.is_none()
            && block_ends_with_array_hint_hole(else_branch)
            && !block_ends_with_array_hint_hole(then_branch)
        {
            let then_type = self.check_block_scoped(then_branch)?;
            let else_type =
                self.check_block_scoped_with_hint(else_branch, array_type_hint(&then_type))?;
            (then_type, else_type)
        } else {
            (
                self.check_block_scoped_with_hint(then_branch, expected)?,
                self.check_block_scoped_with_hint(else_branch, expected)?,
            )
        };

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
    ) -> RainbowResult<Type> {
        if !if_chain_has_final_else(else_branch) {
            let condition_type = self.check_expr(condition)?;
            if condition_type != Type::Bool {
                return Err(expected_type("bool", &condition_type));
            }
            self.check_block_scoped(then_branch)?;
            self.check_block_scoped(else_branch)?;
            return Ok(Type::Unit);
        }

        self.check_if(condition, then_branch, else_branch, None)
    }

    fn check_if_let(
        &mut self,
        pattern: &IfLetPattern,
        value: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        let binding = self.check_if_let_pattern(pattern, value)?;
        let (then_type, else_type) = if expected.is_none()
            && block_ends_with_array_hint_hole(then_branch)
            && !block_ends_with_array_hint_hole(else_branch)
        {
            let else_type = self.check_block_scoped(else_branch)?;
            let then_type =
                self.check_if_let_then(binding, then_branch, array_type_hint(&else_type))?;
            (then_type, else_type)
        } else if expected.is_none()
            && block_ends_with_array_hint_hole(else_branch)
            && !block_ends_with_array_hint_hole(then_branch)
        {
            let then_type = self.check_if_let_then(binding, then_branch, None)?;
            let else_type =
                self.check_block_scoped_with_hint(else_branch, array_type_hint(&then_type))?;
            (then_type, else_type)
        } else {
            (
                self.check_if_let_then(binding, then_branch, expected)?,
                self.check_block_scoped_with_hint(else_branch, expected)?,
            )
        };

        match merge_branch_types(then_type, else_type) {
            Some(ty) => Ok(ty),
            None => Err(type_error("if let branches must have the same type")),
        }
    }

    fn check_if_let_statement(
        &mut self,
        pattern: &IfLetPattern,
        value: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
    ) -> RainbowResult<Type> {
        let binding = self.check_if_let_pattern(pattern, value)?;

        if !if_chain_has_final_else(else_branch) {
            self.check_if_let_then(binding, then_branch, None)?;
            self.check_block_scoped(else_branch)?;
            return Ok(Type::Unit);
        }

        let then_type = self.check_if_let_then(binding, then_branch, None)?;
        let else_type = self.check_block_scoped(else_branch)?;

        match merge_branch_types(then_type, else_type) {
            Some(ty) => Ok(ty),
            None => Err(type_error("if let branches must have the same type")),
        }
    }

    fn check_if_let_pattern<'a>(
        &mut self,
        pattern: &'a IfLetPattern,
        value: &Expr,
    ) -> RainbowResult<Option<(&'a str, Type)>> {
        match pattern {
            IfLetPattern::Binding { name } => match self.check_expr(value)? {
                Type::Nullable(inner) => Ok(Some((name.as_str(), *inner))),
                found => Err(type_error(format!(
                    "if let expected nullable, found {found}"
                ))),
            },
            IfLetPattern::Variant {
                enum_name,
                variant,
                binding,
            } => {
                let value_type = self.check_expr(value)?;
                let Type::Struct(value_enum) = value_type else {
                    return Err(type_error(format!(
                        "if let expected enum value, found {value_type}"
                    )));
                };
                if value_enum != *enum_name {
                    return Err(type_error(format!(
                        "if let pattern expected {value_enum}, found {enum_name}.{variant}"
                    )));
                }

                let variants = self.enums.get(enum_name).ok_or_else(|| {
                    type_error(format!("if let expected enum, found {enum_name}"))
                })?;
                let Some(declared_variant) =
                    variants.iter().find(|declared| declared.name == *variant)
                else {
                    return Err(type_error(format!(
                        "enum '{enum_name}' has no variant '{variant}'"
                    )));
                };

                if let Some(binding) = binding {
                    let Some(payload) = &declared_variant.payload else {
                        return Err(type_error(format!(
                            "if let binding for {enum_name}.{variant} needs a payload variant"
                        )));
                    };
                    Ok(Some((binding.as_str(), payload.as_type())))
                } else {
                    Ok(None)
                }
            }
        }
    }

    fn check_if_let_then(
        &mut self,
        binding: Option<(&str, Type)>,
        then_branch: &[Statement],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        self.push_scope();
        if let Some((name, binding_type)) = binding {
            self.define(name, binding_type, false)?;
        }
        let result = self.check_block_with_hint(then_branch, expected);
        self.pop_scope();
        result
    }

    fn check_match(
        &mut self,
        value: &Expr,
        arms: &[MatchArm],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        let Type::Struct(enum_name) = self.check_expr(value)? else {
            return Err(type_error("match expected an enum value"));
        };
        let variants = self
            .enums
            .get(&enum_name)
            .cloned()
            .ok_or_else(|| type_error(format!("match expected an enum, found {enum_name}")))?;

        let mut seen = HashSet::new();
        let mut saw_else = false;
        let mut checked_arms = Vec::with_capacity(arms.len());

        for arm in arms {
            if saw_else {
                return Err(type_error("match else arm must be last").with_fallback_span(arm.span));
            }

            let mut payload_binding = None;
            match &arm.pattern {
                MatchPattern::Variant {
                    enum_name: arm_enum,
                    variant,
                    binding,
                } => {
                    if arm_enum != &enum_name {
                        return Err(type_error(format!(
                            "match arm expected {enum_name}, found {arm_enum}.{variant}"
                        ))
                        .with_fallback_span(arm.span));
                    }
                    let Some(declared_variant) =
                        variants.iter().find(|declared| declared.name == *variant)
                    else {
                        return Err(type_error(format!(
                            "enum '{enum_name}' has no variant '{variant}'"
                        ))
                        .with_fallback_span(arm.span));
                    };
                    if !seen.insert(variant.clone()) {
                        return Err(type_error(format!(
                            "match has duplicate arm for {enum_name}.{variant}"
                        ))
                        .with_fallback_span(arm.span));
                    }
                    if let Some(binding) = binding {
                        let Some(payload) = &declared_variant.payload else {
                            return Err(type_error(format!(
                                "match arm binding for {enum_name}.{variant} needs a payload variant"
                            ))
                            .with_fallback_span(arm.span));
                        };
                        payload_binding = Some((binding.as_str(), payload.as_type()));
                    }
                }
                MatchPattern::Else => {
                    saw_else = true;
                }
            }

            checked_arms.push((
                arm,
                payload_binding.map(|(binding, ty)| (binding.to_owned(), ty)),
            ));
        }

        if !saw_else {
            for variant in &variants {
                if !seen.contains(&variant.name) {
                    return Err(type_error(format!(
                        "match missing arm for {enum_name}.{}",
                        variant.name
                    )));
                }
            }
        }

        let inferred_array_hint = if expected.is_none() {
            self.infer_match_array_hint(&checked_arms)?
        } else {
            None
        };
        let body_expected = expected.or(inferred_array_hint.as_ref());
        let mut result_type: Option<Type> = None;

        for (arm, payload_binding) in &checked_arms {
            let arm_type =
                self.check_match_arm_body(arm, payload_binding.as_ref(), body_expected)?;
            result_type = Some(match result_type {
                Some(current) => merge_branch_types(current, arm_type).ok_or_else(|| {
                    type_error("match arms must have the same type").with_fallback_span(arm.span)
                })?,
                None => arm_type,
            });
        }

        result_type.ok_or_else(|| type_error("match needs at least one arm"))
    }

    fn infer_match_array_hint(
        &mut self,
        arms: &[(&MatchArm, Option<(String, Type)>)],
    ) -> RainbowResult<Option<Type>> {
        for (arm, payload_binding) in arms {
            if block_ends_with_array_hint_hole(&arm.body) {
                continue;
            }

            let arm_type = self.check_match_arm_body(arm, payload_binding.as_ref(), None)?;
            if matches!(arm_type, Type::Array(_)) {
                return Ok(Some(arm_type));
            }
        }

        Ok(None)
    }

    fn check_match_arm_body(
        &mut self,
        arm: &MatchArm,
        payload_binding: Option<&(String, Type)>,
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        if let Some((binding, ty)) = payload_binding {
            self.push_scope();
            let result = self
                .define(binding, ty.clone(), false)
                .and_then(|_| self.check_block_with_hint(&arm.body, expected));
            self.pop_scope();
            result
        } else {
            self.check_block_scoped_with_hint(&arm.body, expected)
        }
    }

    fn check_return(&mut self, value: Option<&Expr>) -> RainbowResult<Type> {
        let Some(expected) = self.return_types.last().cloned() else {
            return Err(type_error("return outside function"));
        };

        let found = match value {
            Some(value) => self.check_expr_with_hint(value, Some(&expected))?,
            None => Type::Unit,
        };

        if !type_compatible(&expected, &found) && found != Type::Never {
            return Err(type_error(format!(
                "return expected {expected}, found {found}"
            )));
        }

        Ok(Type::Never)
    }

    fn check_while(&mut self, condition: &Expr, body: &[Statement]) -> RainbowResult<()> {
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

    fn check_for(&mut self, name: &str, iterable: &Expr, body: &[Statement]) -> RainbowResult<()> {
        let iterable_type = self.check_expr(iterable)?;
        let element_type = match iterable_type {
            Type::Array(element_type) => *element_type,
            Type::Str => Type::Str,
            found => {
                return Err(type_error(format!(
                    "for loop expected an array or str, found {found}"
                )));
            }
        };

        self.loop_depth += 1;
        self.push_scope();
        self.define(name, element_type, false)?;
        let result = self.check_block(body);
        self.pop_scope();
        self.loop_depth -= 1;
        result?;
        Ok(())
    }

    fn check_block_scoped(&mut self, statements: &[Statement]) -> RainbowResult<Type> {
        self.check_block_scoped_with_hint(statements, None)
    }

    fn check_block_scoped_with_hint(
        &mut self,
        statements: &[Statement],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        self.push_scope();
        let result = self.check_block_with_hint(statements, expected);
        self.pop_scope();
        result
    }

    fn check_block(&mut self, statements: &[Statement]) -> RainbowResult<Type> {
        self.check_block_with_hint(statements, None)
    }

    fn check_block_with_hint(
        &mut self,
        statements: &[Statement],
        expected: Option<&Type>,
    ) -> RainbowResult<Type> {
        let mut last_type = Type::Unit;
        let last_index = statements.len().saturating_sub(1);

        for (index, statement) in statements.iter().enumerate() {
            let result = match statement {
                Statement::Expr { expr, .. } if index == last_index => {
                    self.check_expr_with_hint(expr, expected)
                }
                _ => self.check_statement(statement),
            };

            last_type = result.map_err(|error| {
                error.with_fallback_location(statement.span(), statement.source_path())
            })?;
            if last_type == Type::Never {
                return Ok(Type::Never);
            }
        }

        Ok(last_type)
    }

    fn define(&mut self, name: &str, ty: Type, mutable: bool) -> RainbowResult<()> {
        if self.structs.contains_key(name)
            || self.enums.contains_key(name)
            || self.current_scope().contains_key(name)
        {
            return Err(type_error(format!("binding '{name}' already exists")));
        }

        self.current_scope()
            .insert(name.to_owned(), Binding { ty, mutable });
        Ok(())
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
            TypeName::F64 => Type::F64,
            TypeName::Bool => Type::Bool,
            TypeName::Str => Type::Str,
            TypeName::Unit => Type::Unit,
            TypeName::Struct(name) => Type::Struct(name.clone()),
            TypeName::Array(element) => Type::Array(Box::new(element.as_type())),
            TypeName::Nullable(inner) => Type::Nullable(Box::new(inner.as_type())),
        }
    }
}

impl Checker {
    fn validate_type_name(&self, ty: &TypeName) -> RainbowResult<()> {
        match ty {
            TypeName::Infer
            | TypeName::I64
            | TypeName::F64
            | TypeName::Bool
            | TypeName::Str
            | TypeName::Unit => Ok(()),
            TypeName::Struct(name) if self.structs.contains_key(name) => Ok(()),
            TypeName::Struct(name) if self.enums.contains_key(name) => Ok(()),
            TypeName::Struct(name) => Err(type_error(format!("unknown type '{name}'"))),
            TypeName::Array(element) => self.validate_type_name(element),
            TypeName::Nullable(inner) => self.validate_type_name(inner),
        }
    }
}

fn reject_inferred_signature(
    name: &str,
    params: &[Param],
    return_type: &TypeName,
) -> RainbowResult<()> {
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

fn reject_duplicate_members(
    owner_kind: &str,
    owner_name: &str,
    member_kind: &str,
    members: &[Param],
) -> RainbowResult<()> {
    let mut seen = HashSet::new();

    for member in members {
        if !seen.insert(member.name.as_str()) {
            return Err(type_error(format!(
                "{owner_kind} '{owner_name}' has duplicate {member_kind} '{}'",
                member.name
            )));
        }
    }

    Ok(())
}

fn reject_duplicate_variants(
    owner_kind: &str,
    owner_name: &str,
    member_kind: &str,
    members: &[EnumVariant],
) -> RainbowResult<()> {
    let mut seen = HashSet::new();

    for member in members {
        if !seen.insert(member.name.as_str()) {
            return Err(type_error(format!(
                "{owner_kind} '{owner_name}' has duplicate {member_kind} '{}'",
                member.name
            ))
            .with_fallback_span(member.span));
        }
    }

    Ok(())
}

fn merge_branch_types(left: Type, right: Type) -> Option<Type> {
    match (left, right) {
        (Type::Never, Type::Never) => Some(Type::Never),
        (Type::Never, ty) | (ty, Type::Never) => Some(ty),
        (left, right) if left == right => Some(left),
        (Type::Nullable(inner), found) | (found, Type::Nullable(inner))
            if type_compatible(&Type::Nullable(inner.clone()), &found) =>
        {
            Some(Type::Nullable(inner))
        }
        (Type::Nil, ty) | (ty, Type::Nil) if is_nullable_base_type(&ty) => {
            Some(Type::Nullable(Box::new(ty)))
        }
        _ => None,
    }
}

fn if_chain_has_final_else(else_branch: &[Statement]) -> bool {
    match else_branch {
        [] => false,
        [Statement::If { else_branch, .. }] | [Statement::IfLet { else_branch, .. }] => {
            if_chain_has_final_else(else_branch)
        }
        _ => true,
    }
}

fn is_equatable_type(ty: &Type) -> bool {
    match ty {
        Type::Nil
        | Type::I64
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit
        | Type::Struct(_) => true,
        Type::Array(element) => is_equatable_type(element),
        Type::Nullable(inner) => is_equatable_type(inner),
        Type::Infer | Type::Never | Type::Function { .. } => false,
    }
}

fn equality_compatible(left: &Type, right: &Type) -> bool {
    if left == right {
        return is_equatable_type(left);
    }

    if type_compatible(left, right) {
        return is_equatable_type(left);
    }

    if type_compatible(right, left) {
        return is_equatable_type(right);
    }

    false
}

fn type_compatible(expected: &Type, found: &Type) -> bool {
    if expected == found || *found == Type::Never {
        return true;
    }

    match (expected, found) {
        (Type::Nullable(inner), Type::Nil) => is_nullable_base_type(inner),
        (Type::Nullable(inner), found) => type_compatible(inner, found),
        (Type::Array(expected), Type::Array(found)) => type_compatible(expected, found),
        _ => false,
    }
}

fn is_nullable_base_type(ty: &Type) -> bool {
    !matches!(
        ty,
        Type::Infer | Type::Nil | Type::Never | Type::Function { .. }
    )
}

fn is_empty_array_literal(expr: &Expr) -> bool {
    matches!(expr, Expr::Array(elements) if elements.is_empty())
}

fn expr_needs_array_hint(expr: &Expr) -> bool {
    match expr {
        Expr::Array(elements) => elements.is_empty(),
        Expr::Call { callee, args } if callee == "reverse" && args.len() == 1 => {
            is_empty_array_literal(&args[0])
        }
        _ => false,
    }
}

fn block_ends_with_array_hint_hole(statements: &[Statement]) -> bool {
    matches!(
        statements.last(),
        Some(Statement::Expr { expr, .. }) if expr_needs_array_hint(expr)
    )
}

fn array_type_hint(ty: &Type) -> Option<&Type> {
    matches!(ty, Type::Array(_)).then_some(ty)
}

fn expected_type(expected: &str, found: &Type) -> RainbowError {
    type_error(format!("expected {expected}, found {found}"))
}

fn type_error(message: impl Into<String>) -> RainbowError {
    RainbowError::new(message, Span::new(0, 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn typecheck(source: &str) -> RainbowResult<()> {
        let tokens = lex(source)?;
        let program = parse(&tokens)?;
        check(&program)
    }

    #[test]
    fn type_errors_use_statement_spans() {
        let error = typecheck(
            r#"
let value: i64 = false
"#,
        )
        .expect_err("binding mismatch should fail");

        assert!(error.message.contains("binding 'value' expected i64"));
        assert_eq!((error.line, error.column), (2, 1));
    }

    #[test]
    fn nested_type_errors_use_inner_statement_spans() {
        let error = typecheck(
            r#"
fn broken(value: i64) -> i64:
    let value: str = 3
    return value
"#,
        )
        .expect_err("inner mismatch should fail");

        assert!(error.message.contains("binding 'value' expected str"));
        assert_eq!((error.line, error.column), (3, 5));
    }

    #[test]
    fn match_pattern_errors_use_arm_spans() {
        let error = typecheck(
            r#"
enum Status:
    Ready
enum Mode:
    Ready
let label = match Status.Ready:
    Mode.Ready:
        "mode"
    else:
        "other"
"#,
        )
        .expect_err("wrong enum arm should fail");

        assert!(error.message.contains("match arm expected Status"));
        assert_eq!((error.line, error.column), (7, 5));
    }

    #[test]
    fn match_else_order_errors_use_arm_spans() {
        let error = typecheck(
            r#"
enum Status:
    Ready
let label = match Status.Ready:
    else:
        "other"
    Status.Ready:
        "ready"
"#,
        )
        .expect_err("match arm after else should fail");

        assert!(error.message.contains("match else arm must be last"));
        assert_eq!((error.line, error.column), (7, 5));
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
    fn accepts_local_functions_after_declaration() {
        typecheck(
            r#"
fn outer(value: i64) -> i64:
    fn double(input: i64) -> i64:
        return input * 2

    return double(value)

assert(outer(21) == 42)
"#,
        )
        .expect("local function should typecheck after declaration");
    }

    #[test]
    fn accepts_recursive_local_functions() {
        typecheck(
            r#"
fn outer(value: i64) -> i64:
    fn countdown(n: i64) -> i64:
        if n == 0:
            return value
        else:
            return countdown(n - 1)

    return countdown(3)

assert(outer(42) == 42)
"#,
        )
        .expect("recursive local function should typecheck");
    }

    #[test]
    fn rejects_local_function_before_declaration() {
        let error = typecheck(
            r#"
fn outer(value: i64) -> i64:
    let result = double(value)

    fn double(input: i64) -> i64:
        return input * 2

    return result
"#,
        )
        .expect_err("local function should not be available before declaration");

        assert!(error.message.contains("unknown function 'double'"));
    }

    #[test]
    fn rejects_local_function_name_colliding_with_binding() {
        let error = typecheck(
            r#"
fn outer(value: i64) -> i64:
    let double = value

    fn double(input: i64) -> i64:
        return input * 2

    return double
"#,
        )
        .expect_err("local function collision should fail");

        assert!(error.message.contains("binding 'double' already exists"));
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
    fn rejects_duplicate_function_parameters() {
        let error = typecheck(
            r#"
fn choose(value: i64, value: i64) -> i64:
    return value
"#,
        )
        .expect_err("duplicate parameter should fail");

        assert!(
            error
                .message
                .contains("function 'choose' has duplicate parameter 'value'")
        );
    }

    #[test]
    fn rejects_duplicate_bindings_in_same_scope() {
        let error = typecheck(
            r#"
let answer = 41
let answer = 42
"#,
        )
        .expect_err("duplicate let binding should fail");

        assert!(error.message.contains("binding 'answer' already exists"));
    }

    #[test]
    fn rejects_binding_that_redeclares_parameter() {
        let error = typecheck(
            r#"
fn echo(value: i64) -> i64:
    let value = 42
    return value
"#,
        )
        .expect_err("parameter redeclaration should fail");

        assert!(error.message.contains("binding 'value' already exists"));
    }

    #[test]
    fn rejects_binding_that_redeclares_for_variable() {
        let error = typecheck(
            r#"
for value in [1]:
    let value = 2
"#,
        )
        .expect_err("for variable redeclaration should fail");

        assert!(error.message.contains("binding 'value' already exists"));
    }

    #[test]
    fn rejects_binding_name_colliding_with_function_name() {
        let error = typecheck(
            r#"
fn answer() -> i64:
    return 42

let answer = 42
"#,
        )
        .expect_err("function and binding name collision should fail");

        assert!(error.message.contains("binding 'answer' already exists"));
    }

    #[test]
    fn accepts_shadowing_in_inner_block() {
        typecheck(
            r#"
let value = 1

if true:
    let value = 2
    assert(value == 2)
else:
    assert(value == 1)

assert(value == 1)
"#,
        )
        .expect("inner block shadowing should typecheck");
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
    fn accepts_enum_variants_and_nominal_annotations() {
        typecheck(
            r#"
enum Status:
    Pending
    Ready

fn is_ready(status: Status) -> bool:
    return status == Status.Ready

let status: Status = Status.Ready
let history: [Status] = [Status.Pending, status]
assert(is_ready(status))
assert(contains(history, Status.Pending))
"#,
        )
        .expect("enum program should typecheck");
    }

    #[test]
    fn accepts_exhaustive_enum_match_expressions() {
        typecheck(
            r#"
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
        Status.Failed:
            "failed"

let status = Status.Ready
let label: str = describe(status)
"#,
        )
        .expect("exhaustive enum match should typecheck");
    }

    #[test]
    fn accepts_enum_match_else_fallback() {
        typecheck(
            r#"
enum Status:
    Pending
    Ready
    Failed

fn describe(status: Status) -> str:
    return match status:
        Status.Ready:
            "ready"
        else:
            "not ready"

let failed = describe(Status.Failed)
let pending = describe(Status.Pending)
"#,
        )
        .expect("enum match fallback should typecheck");
    }

    #[test]
    fn accepts_payload_enum_constructors_and_match_bindings() {
        typecheck(
            r#"
enum Result:
    Ok(i64)
    Err(str)

fn unwrap_or_zero(result: Result) -> i64:
    return match result:
        Result.Ok(value):
            value
        Result.Err(message):
            len(message)

let ok: Result = Result.Ok(42)
let err: Result = Result.Err("failed")
let recovered = if let Result.Ok(value) = ok:
    value
else:
    0
let missed = if let Result.Err(message) = ok:
    len(message)
else:
    0
assert(unwrap_or_zero(ok) == 42)
assert(unwrap_or_zero(err) == 6)
assert(recovered == 42)
assert(missed == 0)
"#,
        )
        .expect("payload enum program should typecheck");
    }

    #[test]
    fn rejects_invalid_enum_match_expressions() {
        let non_enum = typecheck(
            r#"
let label = match 1:
    else:
        "one"
"#,
        )
        .expect_err("non-enum match value should fail");
        assert!(non_enum.message.contains("match expected an enum value"));

        let missing = typecheck(
            r#"
enum Status:
    Pending
    Ready

let label = match Status.Ready:
    Status.Ready:
        "ready"
"#,
        )
        .expect_err("missing enum arm should fail");
        assert!(
            missing
                .message
                .contains("match missing arm for Status.Pending")
        );

        let duplicate = typecheck(
            r#"
enum Status:
    Ready

let label = match Status.Ready:
    Status.Ready:
        "ready"
    Status.Ready:
        "again"
"#,
        )
        .expect_err("duplicate enum arm should fail");
        assert!(
            duplicate
                .message
                .contains("match has duplicate arm for Status.Ready")
        );

        let wrong_enum = typecheck(
            r#"
enum Status:
    Ready

enum Mode:
    Ready

let label = match Status.Ready:
    Mode.Ready:
        "ready"
    else:
        "other"
"#,
        )
        .expect_err("wrong enum arm should fail");
        assert!(
            wrong_enum
                .message
                .contains("match arm expected Status, found Mode.Ready")
        );

        let branch_mismatch = typecheck(
            r#"
enum Status:
    Pending
    Ready

let label = match Status.Pending:
    Status.Pending:
        "pending"
    Status.Ready:
        1
"#,
        )
        .expect_err("mismatched match arms should fail");
        assert!(
            branch_mismatch
                .message
                .contains("match arms must have the same type")
        );
    }

    #[test]
    fn rejects_invalid_enum_uses() {
        let duplicate = typecheck(
            r#"
enum Status:
    Ready
    Ready
"#,
        )
        .expect_err("duplicate enum variants should fail");
        assert!(
            duplicate
                .message
                .contains("enum 'Status' has duplicate variant 'Ready'")
        );
        assert_eq!((duplicate.line, duplicate.column), (4, 5));

        let unknown_payload_type = typecheck(
            r#"
enum Result:
    Ok(Missing)
"#,
        )
        .expect_err("unknown enum payload type should fail");
        assert!(
            unknown_payload_type
                .message
                .contains("unknown type 'Missing'")
        );
        assert_eq!(
            (unknown_payload_type.line, unknown_payload_type.column),
            (3, 5)
        );

        let unknown_variant = typecheck(
            r#"
enum Status:
    Ready

let status = Status.Pending
"#,
        )
        .expect_err("unknown enum variant should fail");
        assert!(
            unknown_variant
                .message
                .contains("enum 'Status' has no variant 'Pending'")
        );

        let annotation = typecheck(
            r#"
enum Status:
    Ready

enum Mode:
    Ready

let status: Status = Mode.Ready
"#,
        )
        .expect_err("wrong enum type should fail");
        assert!(
            annotation
                .message
                .contains("binding 'status' expected Status, found Mode")
        );

        let missing_payload = typecheck(
            r#"
enum Result:
    Ok(i64)

let result = Result.Ok
"#,
        )
        .expect_err("missing enum payload should fail");
        assert!(
            missing_payload
                .message
                .contains("enum variant 'Result.Ok' needs a payload")
        );

        let wrong_payload = typecheck(
            r#"
enum Result:
    Ok(i64)

let result = Result.Ok("bad")
"#,
        )
        .expect_err("wrong enum payload should fail");
        assert!(
            wrong_payload
                .message
                .contains("enum variant 'Result.Ok' expected i64, found str")
        );

        let unit_payload = typecheck(
            r#"
enum Status:
    Ready

let status = Status.Ready(1)
"#,
        )
        .expect_err("unit variant payload should fail");
        assert!(
            unit_payload
                .message
                .contains("enum variant 'Status.Ready' does not take a payload")
        );

        let unit_call = typecheck(
            r#"
enum Status:
    Ready

let status = Status.Ready()
"#,
        )
        .expect_err("unit variant call should fail");
        assert!(
            unit_call
                .message
                .contains("enum variant 'Status.Ready' is a value and should not be called")
        );

        let unit_binding = typecheck(
            r#"
enum Status:
    Ready

let label = match Status.Ready:
    Status.Ready(value):
        value
"#,
        )
        .expect_err("unit variant match binding should fail");
        assert!(
            unit_binding
                .message
                .contains("match arm binding for Status.Ready needs a payload variant")
        );

        let unit_if_let_binding = typecheck(
            r#"
enum Status:
    Pending
    Ready

if let Status.Ready(value) = Status.Pending:
    print(value)
"#,
        )
        .expect_err("unit variant if let binding should fail");
        assert!(
            unit_if_let_binding
                .message
                .contains("if let binding for Status.Ready needs a payload variant")
        );
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
    fn rejects_duplicate_struct_fields() {
        let error = typecheck(
            r#"
struct Point:
    x: i64
    x: bool
"#,
        )
        .expect_err("duplicate struct field should fail");

        assert!(
            error
                .message
                .contains("struct 'Point' has duplicate field 'x'")
        );
    }

    #[test]
    fn rejects_function_name_colliding_with_struct_name() {
        let error = typecheck(
            r#"
struct Point:
    x: i64

fn Point() -> i64:
    return 1
"#,
        )
        .expect_err("function and struct name collision should fail");

        assert!(error.message.contains("binding 'Point' already exists"));
    }

    #[test]
    fn rejects_binding_name_colliding_with_struct_name() {
        let error = typecheck(
            r#"
struct Point:
    x: i64

let Point = 42
"#,
        )
        .expect_err("struct and binding name collision should fail");

        assert!(error.message.contains("binding 'Point' already exists"));
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
    fn accepts_string_indexing() {
        typecheck(
            r#"
let name = "Rainbow"
let first: str = name[0]
let second: str = "Rainbow"[1]
assert(first == "F")
assert(second == "y")
"#,
        )
        .expect("string indexing should typecheck");
    }

    #[test]
    fn accepts_string_standard_library() {
        typecheck(
            r#"
let phrase = "  Fast Secure Simple  "
let trimmed: str = trim(phrase)
let lowered: str = lower(trimmed)
let uppered: str = upper(lowered)
let parts: [str] = split(lowered, " ")
let joined: str = join(parts, "-")
let empty_join: str = join([], ",")
let replaced: str = replace(trimmed, "Simple", "Readable")
let starts: bool = starts_with(trimmed, "Fast")
let ends: bool = ends_with(trimmed, "Simple")
assert(len(parts) == 3)
assert(joined == "fast-secure-simple")
assert(empty_join == "")
assert(replaced == "Fast Secure Readable")
assert(starts and ends)
assert(uppered == "FAST SECURE SIMPLE")
"#,
        )
        .expect("string standard library should typecheck");
    }

    #[test]
    fn accepts_flow_calls() {
        typecheck(
            r#"
fn bracket(value: str, left: str, right: str) -> str:
    return left + value + right

let label: str = "  Rainbow  " then trim then lower then bracket("[", "]")
let has_color: bool = label then contains("rainbow")
let size: i64 = label then len
"#,
        )
        .expect("flow calls should typecheck");
    }

    #[test]
    fn rejects_flow_type_errors() {
        let error =
            typecheck("let value = 42 then trim\n").expect_err("flow type error should fail");

        assert!(error.message.contains("trim expected str"));
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
    fn accepts_f64_arithmetic_comparison_and_nullable_recovery() {
        typecheck(
            r#"
fn average(total: f64, count: f64) -> f64:
    return total / count

let radius: f64 = 2.5
let area: f64 = 3.14 * radius * radius
let shifted: f64 = -area + 20.0
let maybe: f64? = nil
let recovered: f64 = maybe ?? average(7.5, 3.0)
assert(area > 19.6 and area < 19.7)
assert(shifted > 0.3)
assert(recovered == 2.5)
"#,
        )
        .expect("f64 arithmetic should typecheck");
    }

    #[test]
    fn accepts_explicit_numeric_conversions() {
        typecheck(
            r#"
let count: i64 = 4
let total: f64 = f64(count) + 2.5
let rounded: i64 = i64(total - 0.5)
assert(rounded == 6)
"#,
        )
        .expect("explicit numeric conversions should typecheck");
    }

    #[test]
    fn rejects_numeric_conversion_type_errors() {
        let bad_input = typecheck("let value = f64(\"3\")\n")
            .expect_err("string conversion should fail at typecheck time");
        assert!(
            bad_input
                .message
                .contains("f64 conversion expects i64 or f64")
        );

        let bad_arity =
            typecheck("let value = i64(1, 2)\n").expect_err("conversion arity should fail");
        assert!(
            bad_arity
                .message
                .contains("i64 expects exactly one argument")
        );
    }

    #[test]
    fn rejects_mixed_numeric_types() {
        let add = typecheck("let value = 1 + 2.0\n").expect_err("mixed add should fail");
        assert!(
            add.message
                .contains("operator 'Add' cannot be applied to i64 and f64")
        );

        let annotation =
            typecheck("let value: f64 = 1\n").expect_err("i64 should not flow into f64");
        assert!(
            annotation
                .message
                .contains("binding 'value' expected f64, found i64")
        );

        let comparison =
            typecheck("let value = 1.0 < 2\n").expect_err("mixed comparison should fail");
        assert!(
            comparison
                .message
                .contains("operator 'Less' cannot be applied to f64 and i64")
        );
    }

    #[test]
    fn accepts_nullable_bindings_arrays_returns_and_comparisons() {
        typecheck(
            r#"
fn maybe(flag: bool) -> i64?:
    if flag:
        return 42
    else:
        return nil

let missing: i64? = nil
let present: i64? = 7
var current: i64? = missing
current = 9
let values: [i64?] = [nil, 1, present]
assert(maybe(true) != nil)
assert(maybe(false) == nil)
assert(contains(values, nil))
assert(contains(values, 1))
"#,
        )
        .expect("nullable values should typecheck");
    }

    #[test]
    fn accepts_coalesce_for_nullable_values() {
        typecheck(
            r#"
fn maybe(flag: bool) -> i64?:
    if flag:
        return 42
    else:
        return nil

let missing: i64? = nil
let present: i64? = 7
let recovered: i64 = missing ?? 10
let chosen: i64 = present ?? 99
let chained: i64 = missing ?? maybe(false) ?? 12
let from_nil: i64 = nil ?? 5
let maybe_ready: bool? = nil
if maybe_ready ?? false:
    assert(false)
else:
    assert(true)
"#,
        )
        .expect("coalesce should safely unwrap nullable values");
    }

    #[test]
    fn accepts_if_let_nullable_narrowing() {
        typecheck(
            r#"
fn maybe(flag: bool) -> i64?:
    if flag:
        return 42
    else:
        return nil

let recovered = if let value = maybe(true):
    value + 1
else:
    0

if let value = maybe(false):
    assert(value > 0)
elif let fallback = maybe(true):
    assert(fallback == 42)
else:
    assert(recovered == 43)

enum Result:
    Ok(i64)
    Err(str)

let ok = Result.Ok(41)
let unwrapped = if let Result.Ok(value) = ok:
    value + 1
else:
    0
assert(unwrapped == 42)

if let Result.Err(message) = ok:
    assert(message == "no")
elif let Result.Ok(value) = ok:
    assert(value == 41)
else:
    assert(false)
"#,
        )
        .expect("if let should narrow nullable values and enum variants");
    }

    #[test]
    fn rejects_invalid_if_let_uses() {
        let non_nullable =
            typecheck("if let value = 42:\n    print(value)\n").expect_err("plain i64 should fail");
        assert!(non_nullable.message.contains("if let expected nullable"));

        let leaked = typecheck(
            r#"
let maybe: i64? = 42
if let value = maybe:
    print(value)
print(value)
"#,
        )
        .expect_err("if let binding should stay scoped to branch");
        assert!(leaked.message.contains("unknown binding 'value'"));

        let mismatched = typecheck(
            r#"
let maybe: i64? = nil
let value = if let found = maybe:
    found
else:
    "missing"
"#,
        )
        .expect_err("if let expression branches should agree");
        assert!(
            mismatched
                .message
                .contains("if let branches must have the same type")
        );

        let non_enum = typecheck(
            r#"
enum Result:
    Ok(i64)

if let Result.Ok(value) = 42:
    print(value)
"#,
        )
        .expect_err("if let enum pattern on non-enum value should fail");
        assert!(non_enum.message.contains("if let expected enum value"));

        let wrong_enum = typecheck(
            r#"
enum Result:
    Ok(i64)

enum Status:
    Ready

if let Status.Ready = Result.Ok(1):
    print("ready")
"#,
        )
        .expect_err("if let wrong enum pattern should fail");
        assert!(
            wrong_enum
                .message
                .contains("if let pattern expected Result, found Status.Ready")
        );

        let unknown_variant = typecheck(
            r#"
enum Result:
    Ok(i64)

let result = Result.Ok(1)
if let Result.Err(message) = result:
    print(message)
"#,
        )
        .expect_err("if let unknown variant should fail");
        assert!(
            unknown_variant
                .message
                .contains("enum 'Result' has no variant 'Err'")
        );

        let unit_binding = typecheck(
            r#"
enum Status:
    Ready

if let Status.Ready(value) = Status.Ready:
    print(value)
"#,
        )
        .expect_err("if let binding on unit variant should fail");
        assert!(
            unit_binding
                .message
                .contains("if let binding for Status.Ready needs a payload variant")
        );
    }

    #[test]
    fn rejects_untyped_and_narrowed_nil_values() {
        let untyped = typecheck("let missing = nil\n").expect_err("untyped nil should fail");
        assert!(untyped.message.contains("explicit nullable type"));

        let non_nullable =
            typecheck("let value: i64 = nil\n").expect_err("nil into i64 should fail");
        assert!(
            non_nullable
                .message
                .contains("binding 'value' expected i64, found nil")
        );

        let narrowed = typecheck(
            r#"
let maybe: i64? = 1
let value: i64 = maybe
"#,
        )
        .expect_err("nullable to plain value should fail");
        assert!(
            narrowed
                .message
                .contains("binding 'value' expected i64, found i64?")
        );
    }

    #[test]
    fn rejects_invalid_coalesce_operands() {
        let plain_left =
            typecheck("let value = 1 ?? 2\n").expect_err("non-null coalesce left side should fail");
        assert!(
            plain_left
                .message
                .contains("coalesce left operand expected nullable")
        );

        let wrong_fallback = typecheck(
            r#"
let maybe: i64? = nil
let value = maybe ?? "missing"
"#,
        )
        .expect_err("wrong fallback should fail");
        assert!(
            wrong_fallback
                .message
                .contains("coalesce fallback expected i64")
        );

        let nullable_result = typecheck(
            r#"
let maybe: i64? = nil
let value: i64 = maybe ?? nil
"#,
        )
        .expect_err("nullable result cannot narrow into i64");
        assert!(
            nullable_result
                .message
                .contains("binding 'value' expected i64, found i64?")
        );
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
    fn accepts_for_loop_over_strings() {
        typecheck(
            r#"
var seen = ""
for ch in "Rainbow":
    let checked: str = ch
    seen = seen + checked

assert(seen == "Rainbow")
"#,
        )
        .expect("string for loop should typecheck");
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
        typecheck("assert(true)\nassert(not false and 1 < 2 or false, \"ordered\")\n")
            .expect("assertions should typecheck");
    }

    #[test]
    fn accepts_elif_chains() {
        typecheck(
            r#"
fn label(value: i64) -> str:
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    elif value == 1:
        return "one"
    else:
        return "many"

assert(label(0) == "zero")
"#,
        )
        .expect("elif branches should typecheck");
    }

    #[test]
    fn accepts_elif_if_expressions() {
        typecheck(
            r#"
let value = 0
let label = if value < 0:
    "negative"
elif value == 0:
    "zero"
else:
    "positive"

assert(label == "zero")
"#,
        )
        .expect("elif expression should typecheck");
    }

    #[test]
    fn rejects_mismatched_elif_expression_branches() {
        let error = typecheck(
            r#"
let value = 0
let label = if value < 0:
    "negative"
elif value == 0:
    0
else:
    "positive"
"#,
        )
        .expect_err("mismatched elif expression should fail");

        assert!(error.message.contains("branches"));
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
    fn accepts_contains() {
        typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let points = [Point { x: 3, y: 4 }]

assert(contains([1, 2, 3], 2))
assert(contains("secure Rainbow", "Rainbow"))
assert(contains(points, Point { x: 3, y: 4 }))
"#,
        )
        .expect("contains should typecheck");
    }

    #[test]
    fn accepts_slice_for_arrays_and_strings() {
        typecheck(
            r#"
let values = [3, 5, 8, 13, 21]
let middle: [i64] = slice(values, 1, 4)
let prefix: str = slice("secure Rainbow", 0, 6)
assert(middle == [5, 8, 13])
assert(prefix == "secure")
"#,
        )
        .expect("slice should typecheck");
    }

    #[test]
    fn accepts_is_empty_for_arrays_and_strings() {
        typecheck(
            r#"
let values = [3, 5, 8]
assert(is_empty([]))
assert(not is_empty(values))
assert(is_empty(""))
assert(not is_empty("Rainbow"))
"#,
        )
        .expect("is_empty should typecheck");
    }

    #[test]
    fn accepts_get_with_fallbacks() {
        typecheck(
            r#"
let values = [3, 5, 8]
let found: i64 = get(values, 1, -1)
let fallback = get(values, 99, -1)
let inferred = get([], 0, 42)
let letter: str = get("Rainbow", 1, "?")
assert(found == 5)
assert(fallback == -1)
assert(inferred == 42)
assert(letter == "y")
"#,
        )
        .expect("get should typecheck");
    }

    #[test]
    fn accepts_reverse_first_and_last_for_arrays_and_strings() {
        typecheck(
            r#"
let values = [3, 5, 8]
let reversed: [i64] = reverse(values)
let reversed_empty: [i64] = reverse([])
let first_value: i64 = first(values, -1)
let last_value: i64 = last(values, -1)
let inferred_first = first([], 42)
let inferred_last = last([], 42)
let rows = [[1, 2]]
let first_row: [i64] = first(rows, [])
let fallback_row: [i64] = first([], [])
let reversed_text: str = reverse("Rainbow")
let first_letter: str = first("Rainbow", "?")
let last_letter: str = last("Rainbow", "?")
assert(reversed == [8, 5, 3])
assert(len(reversed_empty) == 0)
assert(first_value == 3)
assert(last_value == 8)
assert(inferred_first == 42)
assert(inferred_last == 42)
assert(first_row == [1, 2])
assert(len(fallback_row) == 0)
assert(reversed_text == "wobniaR")
assert(first_letter == "F")
assert(last_letter == "r")
"#,
        )
        .expect("reverse/first/last should typecheck");
    }

    #[test]
    fn accepts_find_for_arrays_and_strings() {
        typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let values = [3, 5, 8]
let points = [Point { x: 3, y: 4 }]
let empty_index = find([], 21)
let value_index = find(values, 5)
let point_index = find(points, Point { x: 3, y: 4 })
let text_index = find("secure Rainbow", "Rainbow")
assert(empty_index == -1)
assert(value_index == 1)
assert(point_index == 0)
assert(text_index == 7)
"#,
        )
        .expect("find should typecheck");
    }

    #[test]
    fn rejects_find_collection_type_mismatch() {
        let error = typecheck("find(42, 1)\n").expect_err("find collection should fail");

        assert!(error.message.contains("find expects an array or str"));
    }

    #[test]
    fn rejects_find_value_type_mismatch() {
        let array_error =
            typecheck("find([1, 2, 3], true)\n").expect_err("array needle should fail");
        assert!(array_error.message.contains("find expected i64"));

        let string_error = typecheck("find(\"Rainbow\", 1)\n").expect_err("str needle should fail");
        assert!(
            string_error
                .message
                .contains("find(str, value) expected str")
        );
    }

    #[test]
    fn rejects_wrong_find_arity() {
        let error = typecheck("find([1, 2, 3])\n").expect_err("find arity should fail");

        assert!(error.message.contains("find expects exactly two arguments"));
    }

    #[test]
    fn accepts_count_for_arrays_and_strings() {
        typecheck(
            r#"
struct Point:
    x: i64
    y: i64

let values = [3, 5, 3, 8, 3]
let points = [Point { x: 3, y: 4 }]
let empty_count = count([], 21)
let value_count = count(values, 3)
let point_count = count(points, Point { x: 3, y: 4 })
let text_count = count("secure Rainbow secure", "secure")
assert(empty_count == 0)
assert(value_count == 3)
assert(point_count == 1)
assert(text_count == 2)
"#,
        )
        .expect("count should typecheck");
    }

    #[test]
    fn rejects_count_collection_type_mismatch() {
        let error = typecheck("count(42, 1)\n").expect_err("count collection should fail");

        assert!(error.message.contains("count expects an array or str"));
    }

    #[test]
    fn rejects_count_value_type_mismatch() {
        let array_error =
            typecheck("count([1, 2, 3], true)\n").expect_err("array needle should fail");
        assert!(array_error.message.contains("count expected i64"));

        let string_error =
            typecheck("count(\"Rainbow\", 1)\n").expect_err("str needle should fail");
        assert!(
            string_error
                .message
                .contains("count(str, value) expected str")
        );
    }

    #[test]
    fn rejects_wrong_count_arity() {
        let error = typecheck("count([1, 2, 3])\n").expect_err("count arity should fail");

        assert!(
            error
                .message
                .contains("count expects exactly two arguments")
        );
    }

    #[test]
    fn accepts_get_with_empty_array_default_hint() {
        typecheck(
            r#"
let rows = [[1, 2]]
let first_row = get(rows, 0, [])
let fallback: [i64] = get([], 0, [])
assert(first_row == [1, 2])
assert(len(fallback) == 0)
"#,
        )
        .expect("get should hint empty array defaults");
    }

    #[test]
    fn rejects_get_collection_type_mismatch() {
        let error = typecheck("get(42, 0, 1)\n").expect_err("get collection should fail");

        assert!(error.message.contains("get expects an array or str"));
    }

    #[test]
    fn rejects_get_index_type_mismatch() {
        let error = typecheck("get([1, 2, 3], true, 0)\n").expect_err("get index should fail");

        assert!(error.message.contains("get index expected i64"));
    }

    #[test]
    fn rejects_get_default_type_mismatch() {
        let array_error =
            typecheck("get([1, 2, 3], 0, true)\n").expect_err("array default should fail");
        assert!(array_error.message.contains("get default expected i64"));

        let string_error =
            typecheck("get(\"Rainbow\", 0, 0)\n").expect_err("str default should fail");
        assert!(string_error.message.contains("get default expected str"));
    }

    #[test]
    fn rejects_wrong_get_arity() {
        let error = typecheck("get([1, 2, 3], 0)\n").expect_err("get arity should fail");

        assert!(
            error
                .message
                .contains("get expects exactly three arguments")
        );
    }

    #[test]
    fn rejects_reverse_type_errors() {
        let collection = typecheck("reverse(42)\n").expect_err("reverse collection should fail");
        assert!(
            collection
                .message
                .contains("reverse expects an array or str")
        );

        let empty = typecheck("reverse([])\n").expect_err("reverse empty array should fail");
        assert!(
            empty
                .message
                .contains("reverse needs an expected array type")
        );

        let arity = typecheck("reverse([1], [2])\n").expect_err("reverse arity should fail");
        assert!(
            arity
                .message
                .contains("reverse expects exactly one argument")
        );
    }

    #[test]
    fn rejects_first_and_last_type_errors() {
        let first_collection =
            typecheck("first(42, 0)\n").expect_err("first collection should fail");
        assert!(
            first_collection
                .message
                .contains("first expects an array or str")
        );

        let last_collection = typecheck("last(42, 0)\n").expect_err("last collection should fail");
        assert!(
            last_collection
                .message
                .contains("last expects an array or str")
        );

        let first_default =
            typecheck("first([1, 2, 3], true)\n").expect_err("first default should fail");
        assert!(first_default.message.contains("first default expected i64"));

        let last_default =
            typecheck("last(\"Rainbow\", 0)\n").expect_err("last default should fail");
        assert!(last_default.message.contains("last default expected str"));

        let arity = typecheck("first([1, 2, 3])\n").expect_err("first arity should fail");
        assert!(
            arity
                .message
                .contains("first expects exactly two arguments")
        );
    }

    #[test]
    fn rejects_is_empty_type_mismatch() {
        let error = typecheck("is_empty(42)\n").expect_err("is_empty type should fail");

        assert!(error.message.contains("is_empty expects an array or str"));
    }

    #[test]
    fn rejects_wrong_is_empty_arity() {
        let error = typecheck("is_empty([], [])\n").expect_err("is_empty arity should fail");

        assert!(
            error
                .message
                .contains("is_empty expects exactly one argument")
        );
    }

    #[test]
    fn rejects_slice_collection_type_mismatch() {
        let error = typecheck("slice(42, 0, 1)\n").expect_err("slice collection should fail");

        assert!(error.message.contains("slice expects an array or str"));
    }

    #[test]
    fn rejects_slice_index_type_mismatch() {
        let error = typecheck("slice([1, 2, 3], true, 2)\n").expect_err("slice index should fail");

        assert!(error.message.contains("slice start expected i64"));
    }

    #[test]
    fn rejects_wrong_slice_arity() {
        let error = typecheck("slice([1, 2, 3], 1)\n").expect_err("slice arity should fail");

        assert!(
            error
                .message
                .contains("slice expects exactly three arguments")
        );
    }

    #[test]
    fn rejects_contains_value_type_mismatch() {
        let error =
            typecheck("contains([1, 2, 3], true)\n").expect_err("contains mismatch should fail");

        assert!(error.message.contains("contains expected i64"));
    }

    #[test]
    fn rejects_contains_string_needle_type_mismatch() {
        let error = typecheck("contains(\"rainbow\", 1)\n")
            .expect_err("contains string mismatch should fail");

        assert!(error.message.contains("contains(str, value) expected str"));
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
    fn accepts_array_append() {
        typecheck(
            r#"
let values = append([3, 5, 8], 13)
let more_values = append(values, 21)
assert(more_values == [3, 5, 8, 13, 21])
"#,
        )
        .expect("append should typecheck");
    }

    #[test]
    fn infers_append_from_empty_array_literal() {
        typecheck(
            r#"
let values = append([], 1)
let nested: [[i64]] = append([], [])
assert(values == [1])
assert(len(nested) == 1)
assert(len(nested[0]) == 0)
"#,
        )
        .expect("append should infer empty array element type");
    }

    #[test]
    fn threads_expected_array_types_into_branch_expressions() {
        typecheck(
            r#"
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

assert(is_empty(from_if))
assert(is_empty(from_if_let))
assert(is_empty(from_match))
"#,
        )
        .expect("expected branch result types should type empty arrays");
    }

    #[test]
    fn infers_empty_array_branch_types_from_sibling_branches() {
        typecheck(
            r#"
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

assert(is_empty(from_if))
assert(is_empty(from_reverse))
assert(is_empty(from_if_let))
assert(is_empty(from_match))
"#,
        )
        .expect("sibling branch array types should type empty array branches");
    }

    #[test]
    fn rejects_append_collection_type_mismatch() {
        let error = typecheck("append(42, 1)\n").expect_err("append collection should fail");

        assert!(error.message.contains("append expects an array"));
    }

    #[test]
    fn rejects_append_value_type_mismatch() {
        let error = typecheck("append([1, 2, 3], true)\n").expect_err("append value should fail");

        assert!(error.message.contains("append expected i64"));
    }

    #[test]
    fn rejects_wrong_append_arity() {
        let error = typecheck("append([1, 2, 3])\n").expect_err("append arity should fail");

        assert!(
            error
                .message
                .contains("append expects exactly two arguments")
        );
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
    fn rejects_invalid_string_indexing() {
        let index = typecheck("\"Rainbow\"[true]\n").expect_err("string index should fail");
        assert!(index.message.contains("string index expected i64"));

        let collection = typecheck("42[0]\n").expect_err("primitive indexing should fail");
        assert!(
            collection
                .message
                .contains("indexing expected an array or str")
        );
    }

    #[test]
    fn rejects_string_standard_library_type_errors() {
        let transform = typecheck("trim(42)\n").expect_err("trim type should fail");
        assert!(transform.message.contains("trim expected str"));

        let predicate =
            typecheck("starts_with(\"Rainbow\", 1)\n").expect_err("starts_with type should fail");
        assert!(predicate.message.contains("starts_with value expected str"));

        let replace = typecheck("replace(\"Rainbow\", \"y\", 1)\n")
            .expect_err("replace new type should fail");
        assert!(replace.message.contains("replace new expected str"));

        let split = typecheck("split(\"Rainbow\", 1)\n").expect_err("split separator should fail");
        assert!(split.message.contains("split separator expected str"));

        let join_collection =
            typecheck("join(42, \",\")\n").expect_err("join collection type should fail");
        assert!(join_collection.message.contains("join expects [str]"));

        let join_element =
            typecheck("join([1], \",\")\n").expect_err("join element type should fail");
        assert!(join_element.message.contains("join expects [str]"));

        let arity = typecheck("split(\"Rainbow\")\n").expect_err("split arity should fail");
        assert!(
            arity
                .message
                .contains("split expects exactly two arguments")
        );
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

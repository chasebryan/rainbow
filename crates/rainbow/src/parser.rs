use crate::ast::{
    BinaryOp, EnumVariant, Expr, IfLetPattern, MatchArm, MatchPattern, Param, Program, Statement,
    TypeName, UnaryOp,
};
use crate::diagnostic::{RainbowError, RainbowResult};
use crate::lexer::{Token, TokenKind};
use crate::span::Span;

pub fn parse(tokens: &[Token]) -> RainbowResult<Program> {
    Parser::new(tokens).parse()
}

struct Parser<'a> {
    tokens: &'a [Token],
    current: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, current: 0 }
    }

    fn parse(mut self) -> RainbowResult<Program> {
        let mut statements = Vec::new();

        self.skip_newlines();
        while !self.is_at_end() {
            statements.push(self.statement()?);
            self.consume_statement_separator()?;
        }

        Ok(Program { statements })
    }

    fn statement(&mut self) -> RainbowResult<Statement> {
        if self.match_kind(&TokenKind::Let) {
            return self.let_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Var) {
            return self.var_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Fn) {
            return self.fn_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Struct) {
            return self.struct_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Enum) {
            return self.enum_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Import) {
            return self.import_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::While) {
            return self.while_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::For) {
            return self.for_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::If) {
            return self.if_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Return) {
            return self.return_statement(self.previous().span);
        }

        if self.match_kind(&TokenKind::Break) {
            return Ok(Statement::Break {
                span: self.previous().span,
                source_path: None,
            });
        }

        if self.match_kind(&TokenKind::Continue) {
            return Ok(Statement::Continue {
                span: self.previous().span,
                source_path: None,
            });
        }

        if self.check_identifier_assignment() {
            return self.assignment_statement();
        }

        let span = self.peek().span;
        Ok(Statement::Expr {
            expr: self.expression()?,
            span,
            source_path: None,
        })
    }

    fn let_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected an identifier after 'let'",
                    self.previous().span,
                ));
            }
        };

        let ty = self.optional_type_annotation()?;
        self.consume(&TokenKind::Equal, "expected '=' after binding name")?;
        let value = self.expression()?;

        Ok(Statement::Let {
            name,
            ty,
            value,
            span,
            source_path: None,
        })
    }

    fn var_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected an identifier after 'var'",
                    self.previous().span,
                ));
            }
        };

        let ty = self.optional_type_annotation()?;
        self.consume(&TokenKind::Equal, "expected '=' after mutable binding name")?;
        let value = self.expression()?;

        Ok(Statement::Var {
            name,
            ty,
            value,
            span,
            source_path: None,
        })
    }

    fn assignment_statement(&mut self) -> RainbowResult<Statement> {
        let token = self.advance();
        let name = match &token.kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => unreachable!("caller checks assignment shape"),
        };

        self.consume(&TokenKind::Equal, "expected '=' in assignment")?;
        let value = self.expression()?;

        Ok(Statement::Assign {
            name,
            value,
            span: token.span,
            source_path: None,
        })
    }

    fn fn_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected a function name after 'fn'",
                    self.previous().span,
                ));
            }
        };

        self.consume(&TokenKind::LParen, "expected '(' after function name")?;
        let params = self.parameter_list()?;
        self.consume(&TokenKind::RParen, "expected ')' after function parameters")?;
        let return_type = if self.match_kind(&TokenKind::Arrow) {
            self.type_name()?
        } else {
            TypeName::Infer
        };
        self.consume(&TokenKind::Colon, "expected ':' before function body")?;
        let body = self.block("function body")?;

        Ok(Statement::Fn {
            name,
            params,
            return_type,
            body,
            span,
            source_path: None,
        })
    }

    fn struct_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected a struct name after 'struct'",
                    self.previous().span,
                ));
            }
        };

        self.consume(&TokenKind::Colon, "expected ':' before struct fields")?;
        let fields = self.struct_fields()?;

        Ok(Statement::Struct {
            name,
            fields,
            span,
            source_path: None,
        })
    }

    fn enum_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected an enum name after 'enum'",
                    self.previous().span,
                ));
            }
        };

        self.consume(&TokenKind::Colon, "expected ':' before enum variants")?;
        let variants = self.enum_variants()?;

        Ok(Statement::Enum {
            name,
            variants,
            span,
            source_path: None,
        })
    }

    fn import_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let path = match &self.advance().kind {
            TokenKind::Str(path) => path.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected a string path after 'import'",
                    self.previous().span,
                ));
            }
        };

        Ok(Statement::Import {
            path,
            span,
            source_path: None,
        })
    }

    fn struct_fields(&mut self) -> RainbowResult<Vec<Param>> {
        self.consume(
            &TokenKind::Newline,
            "expected a newline before struct fields",
        )?;
        self.consume(&TokenKind::Indent, "expected indented struct fields")?;
        self.skip_newlines();

        let mut fields = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            let field_name = match &self.advance().kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new(
                        "expected a field name in struct",
                        self.previous().span,
                    ));
                }
            };

            self.consume(&TokenKind::Colon, "expected ':' after field name")?;
            let ty = self.type_name()?;
            fields.push(Param {
                name: field_name,
                ty,
            });
            self.consume_statement_separator()?;
        }

        if fields.is_empty() {
            return Err(RainbowError::new(
                "expected at least one field in struct",
                self.peek().span,
            ));
        }

        self.consume(&TokenKind::Dedent, "expected struct fields to dedent")?;
        Ok(fields)
    }

    fn enum_variants(&mut self) -> RainbowResult<Vec<EnumVariant>> {
        self.consume(
            &TokenKind::Newline,
            "expected a newline before enum variants",
        )?;
        self.consume(&TokenKind::Indent, "expected indented enum variants")?;
        self.skip_newlines();

        let mut variants = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            let token = self.advance();
            let span = token.span;
            let name = match &token.kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new("expected a variant name in enum", span));
                }
            };

            let payload = if self.match_kind(&TokenKind::LParen) {
                let ty = self.type_name()?;
                self.consume(
                    &TokenKind::RParen,
                    "expected ')' after variant payload type",
                )?;
                Some(ty)
            } else {
                None
            };

            variants.push(EnumVariant {
                name,
                payload,
                span,
            });
            self.consume_statement_separator()?;
        }

        if variants.is_empty() {
            return Err(RainbowError::new(
                "expected at least one variant in enum",
                self.peek().span,
            ));
        }

        self.consume(&TokenKind::Dedent, "expected enum variants to dedent")?;
        Ok(variants)
    }

    fn while_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let condition = self.coalesce()?;
        self.consume(&TokenKind::Colon, "expected ':' after while condition")?;
        let body = self.block("while body")?;

        Ok(Statement::While {
            condition,
            body,
            span,
            source_path: None,
        })
    }

    fn for_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected an identifier after 'for'",
                    self.previous().span,
                ));
            }
        };

        self.consume(&TokenKind::In, "expected 'in' after for-loop binding")?;
        let iterable = self.expression()?;
        self.consume(&TokenKind::Colon, "expected ':' after for-loop iterable")?;
        let body = self.block("for body")?;

        Ok(Statement::For {
            name,
            iterable,
            body,
            span,
            source_path: None,
        })
    }

    fn if_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        if self.match_kind(&TokenKind::Let) {
            let (pattern, value) = self.if_let_header()?;
            let then_branch = self.block("if let body")?;
            let else_branch = self.if_statement_tail()?;

            return Ok(Statement::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
                span,
                source_path: None,
            });
        }

        let condition = self.coalesce()?;
        self.consume(&TokenKind::Colon, "expected ':' after if condition")?;
        let then_branch = self.block("if body")?;
        let else_branch = self.if_statement_tail()?;

        Ok(Statement::If {
            condition,
            then_branch,
            else_branch,
            span,
            source_path: None,
        })
    }

    fn return_statement(&mut self, span: Span) -> RainbowResult<Statement> {
        let value = if self.check_statement_boundary() {
            None
        } else {
            Some(self.expression()?)
        };

        Ok(Statement::Return {
            value,
            span,
            source_path: None,
        })
    }

    fn parameter_list(&mut self) -> RainbowResult<Vec<Param>> {
        let mut params = Vec::new();

        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }

        loop {
            let token = self.advance();
            let name = match &token.kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new(
                        "expected a parameter name",
                        self.previous().span,
                    ));
                }
            };

            let ty = if self.match_kind(&TokenKind::Colon) {
                self.type_name()?
            } else {
                TypeName::Infer
            };
            params.push(Param { name, ty });

            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }

        Ok(params)
    }

    fn optional_type_annotation(&mut self) -> RainbowResult<TypeName> {
        if self.match_kind(&TokenKind::Colon) {
            self.type_name()
        } else {
            Ok(TypeName::Infer)
        }
    }

    fn type_name(&mut self) -> RainbowResult<TypeName> {
        let mut ty = self.primary_type_name()?;

        while self.match_kind(&TokenKind::Question) {
            ty = TypeName::Nullable(Box::new(ty));
        }

        Ok(ty)
    }

    fn primary_type_name(&mut self) -> RainbowResult<TypeName> {
        let token = self.advance();

        match &token.kind {
            TokenKind::LBracket => {
                let element = self.type_name()?;
                self.consume(&TokenKind::RBracket, "expected ']' after array type")?;
                Ok(TypeName::Array(Box::new(element)))
            }
            TokenKind::Identifier(name) if name == "i64" => Ok(TypeName::I64),
            TokenKind::Identifier(name) if name == "f64" => Ok(TypeName::F64),
            TokenKind::Identifier(name) if name == "bool" => Ok(TypeName::Bool),
            TokenKind::Identifier(name) if name == "str" => Ok(TypeName::Str),
            TokenKind::Identifier(name) if name == "unit" => Ok(TypeName::Unit),
            TokenKind::Identifier(name) => Ok(TypeName::Struct(name.clone())),
            _ => Err(RainbowError::new("expected a type name", token.span)),
        }
    }

    fn expression(&mut self) -> RainbowResult<Expr> {
        if self.match_kind(&TokenKind::If) {
            return self.if_expression();
        }

        if self.match_kind(&TokenKind::Match) {
            return self.match_expression();
        }

        self.coalesce()
    }

    fn if_expression(&mut self) -> RainbowResult<Expr> {
        if self.match_kind(&TokenKind::Let) {
            let (pattern, value) = self.if_let_header()?;
            let then_branch = self.block("if let body")?;
            let else_branch = self.if_expression_tail()?;

            return Ok(Expr::IfLet {
                pattern,
                value: Box::new(value),
                then_branch,
                else_branch,
            });
        }

        let condition = self.coalesce()?;
        self.consume(&TokenKind::Colon, "expected ':' after if condition")?;
        let then_branch = self.block("if body")?;
        let else_branch = self.if_expression_tail()?;

        Ok(Expr::If {
            condition: Box::new(condition),
            then_branch,
            else_branch,
        })
    }

    fn if_statement_tail(&mut self) -> RainbowResult<Vec<Statement>> {
        self.skip_newlines();

        if self.match_kind(&TokenKind::Elif) {
            return self.elif_tail(false, self.previous().span);
        }

        if self.match_kind(&TokenKind::Else) {
            self.consume(&TokenKind::Colon, "expected ':' after else")?;
            return self.block("else body");
        }

        Ok(Vec::new())
    }

    fn if_expression_tail(&mut self) -> RainbowResult<Vec<Statement>> {
        self.skip_newlines();

        if self.match_kind(&TokenKind::Elif) {
            return self.elif_tail(true, self.previous().span);
        }

        self.consume(&TokenKind::Else, "expected 'else' branch for if expression")?;
        self.consume(&TokenKind::Colon, "expected ':' after else")?;
        self.block("else body")
    }

    fn elif_tail(&mut self, require_else: bool, span: Span) -> RainbowResult<Vec<Statement>> {
        if self.match_kind(&TokenKind::Let) {
            let (pattern, value) = self.if_let_header()?;
            let then_branch = self.block("elif let body")?;
            let else_branch = if require_else {
                self.if_expression_tail()?
            } else {
                self.if_statement_tail()?
            };

            return Ok(vec![Statement::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
                span,
                source_path: None,
            }]);
        }

        let condition = self.coalesce()?;
        self.consume(&TokenKind::Colon, "expected ':' after elif condition")?;
        let then_branch = self.block("elif body")?;
        let else_branch = if require_else {
            self.if_expression_tail()?
        } else {
            self.if_statement_tail()?
        };

        Ok(vec![Statement::If {
            condition,
            then_branch,
            else_branch,
            span,
            source_path: None,
        }])
    }

    fn match_expression(&mut self) -> RainbowResult<Expr> {
        let value = self.coalesce()?;
        self.consume(&TokenKind::Colon, "expected ':' after match value")?;
        let arms = self.match_arms()?;

        Ok(Expr::Match {
            value: Box::new(value),
            arms,
        })
    }

    fn match_arms(&mut self) -> RainbowResult<Vec<MatchArm>> {
        self.consume(&TokenKind::Newline, "expected a newline before match arms")?;
        self.consume(&TokenKind::Indent, "expected indented match arms")?;
        self.skip_newlines();

        let mut arms = Vec::new();
        let mut saw_else = false;
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            let span = self.peek().span;
            if saw_else {
                return Err(RainbowError::new(
                    "match else arm must be last",
                    self.peek().span,
                ));
            }

            let pattern = self.match_pattern()?;
            saw_else = matches!(pattern, MatchPattern::Else);
            self.consume(&TokenKind::Colon, "expected ':' after match arm pattern")?;
            let body = self.block("match arm")?;
            arms.push(MatchArm {
                pattern,
                body,
                span,
            });
            self.skip_newlines();
        }

        if arms.is_empty() {
            return Err(RainbowError::new(
                "expected at least one arm in match",
                self.peek().span,
            ));
        }

        self.consume(&TokenKind::Dedent, "expected match arms to dedent")?;
        Ok(arms)
    }

    fn match_pattern(&mut self) -> RainbowResult<MatchPattern> {
        if self.match_kind(&TokenKind::Else) {
            return Ok(MatchPattern::Else);
        }

        let enum_name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected an enum variant or else in match arm",
                    self.previous().span,
                ));
            }
        };
        self.consume(&TokenKind::Dot, "expected '.' in enum match arm")?;
        let variant = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected a variant name in match arm",
                    self.previous().span,
                ));
            }
        };

        let binding = if self.match_kind(&TokenKind::LParen) {
            let token = self.advance();
            let binding = match &token.kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new(
                        "expected a payload binding name in match arm",
                        token.span,
                    ));
                }
            };
            self.consume(
                &TokenKind::RParen,
                "expected ')' after match payload binding",
            )?;
            Some(binding)
        } else {
            None
        };

        Ok(MatchPattern::Variant {
            enum_name,
            variant,
            binding,
        })
    }

    fn if_let_header(&mut self) -> RainbowResult<(IfLetPattern, Expr)> {
        let token = self.advance();
        let first_name = match &token.kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(RainbowError::new(
                    "expected a binding name or enum variant after let",
                    token.span,
                ));
            }
        };

        let pattern = if self.match_kind(&TokenKind::Dot) {
            let variant = match &self.advance().kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new(
                        "expected a variant name in if let pattern",
                        self.previous().span,
                    ));
                }
            };
            let binding = if self.match_kind(&TokenKind::LParen) {
                let token = self.advance();
                let binding = match &token.kind {
                    TokenKind::Identifier(name) => name.clone(),
                    _ => {
                        return Err(RainbowError::new(
                            "expected a payload binding name in if let pattern",
                            token.span,
                        ));
                    }
                };
                self.consume(
                    &TokenKind::RParen,
                    "expected ')' after if let payload binding",
                )?;
                Some(binding)
            } else {
                None
            };
            IfLetPattern::Variant {
                enum_name: first_name,
                variant,
                binding,
            }
        } else {
            IfLetPattern::Binding { name: first_name }
        };

        self.consume(&TokenKind::Equal, "expected '=' after if let binding")?;
        let value = self.coalesce()?;
        self.consume(&TokenKind::Colon, "expected ':' after if let value")?;
        Ok((pattern, value))
    }

    fn coalesce(&mut self) -> RainbowResult<Expr> {
        let expr = self.flow()?;

        if self.match_kind(&TokenKind::QuestionQuestion) {
            let right = self.coalesce()?;
            return Ok(Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Coalesce,
                right: Box::new(right),
            });
        }

        Ok(expr)
    }

    fn flow(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.or()?;

        while self.match_kind(&TokenKind::Then) {
            let callee = match &self.advance().kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new(
                        "expected a function name after flow keyword",
                        self.previous().span,
                    ));
                }
            };

            let mut args = Vec::new();
            if self.match_kind(&TokenKind::LParen) {
                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.expression()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }
                self.consume(&TokenKind::RParen, "expected ')' after flow arguments")?;
            }

            expr = Expr::Flow {
                value: Box::new(expr),
                callee,
                args,
            };
        }

        Ok(expr)
    }

    fn or(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.and()?;

        while self.match_kind(&TokenKind::OrOr) {
            let right = self.and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn and(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.equality()?;

        while self.match_kind(&TokenKind::AndAnd) {
            let right = self.equality()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn equality(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.comparison()?;

        while let Some(op) = self.match_binary(&[
            (TokenKind::EqualEqual, BinaryOp::Equal),
            (TokenKind::BangEqual, BinaryOp::NotEqual),
        ]) {
            let right = self.comparison()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn comparison(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.term()?;

        while let Some(op) = self.match_binary(&[
            (TokenKind::Less, BinaryOp::Less),
            (TokenKind::LessEqual, BinaryOp::LessEqual),
            (TokenKind::Greater, BinaryOp::Greater),
            (TokenKind::GreaterEqual, BinaryOp::GreaterEqual),
        ]) {
            let right = self.term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn term(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.factor()?;

        while let Some(op) = self.match_binary(&[
            (TokenKind::Plus, BinaryOp::Add),
            (TokenKind::Minus, BinaryOp::Subtract),
        ]) {
            let right = self.factor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn factor(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.unary()?;

        while let Some(op) = self.match_binary(&[
            (TokenKind::Star, BinaryOp::Multiply),
            (TokenKind::Slash, BinaryOp::Divide),
            (TokenKind::Percent, BinaryOp::Remainder),
        ]) {
            let right = self.unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn unary(&mut self) -> RainbowResult<Expr> {
        if self.match_kind(&TokenKind::Bang) {
            let expr = self.unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                expr: Box::new(expr),
            });
        }

        if self.match_kind(&TokenKind::Minus) {
            let expr = self.unary()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Negate,
                expr: Box::new(expr),
            });
        }

        self.call()
    }

    fn call(&mut self) -> RainbowResult<Expr> {
        let mut expr = self.primary()?;

        loop {
            if self.match_kind(&TokenKind::LParen) {
                let mut args = Vec::new();
                if !self.check(&TokenKind::RParen) {
                    loop {
                        args.push(self.expression()?);
                        if !self.match_kind(&TokenKind::Comma) {
                            break;
                        }
                    }
                }

                self.consume(&TokenKind::RParen, "expected ')' after function arguments")?;
                expr = match expr {
                    Expr::Variable(callee) => Expr::Call { callee, args },
                    Expr::Field { object, field } => {
                        let Expr::Variable(enum_name) = *object else {
                            return Err(RainbowError::new(
                                "only named functions and enum variants can be called in Rainbow bootstrap",
                                self.previous().span,
                            ));
                        };
                        if args.len() > 1 {
                            return Err(RainbowError::new(
                                "enum variant constructors take at most one payload",
                                self.previous().span,
                            ));
                        }
                        Expr::EnumInit {
                            enum_name,
                            variant: field,
                            value: args.into_iter().next().map(Box::new),
                        }
                    }
                    _ => {
                        return Err(RainbowError::new(
                            "only named functions and enum variants can be called in Rainbow bootstrap",
                            self.previous().span,
                        ));
                    }
                };
                continue;
            }

            if self.match_kind(&TokenKind::LBrace) {
                let Expr::Variable(name) = expr else {
                    return Err(RainbowError::new(
                        "expected a struct name before '{'",
                        self.previous().span,
                    ));
                };
                let fields = self.struct_initializer_fields()?;
                expr = Expr::StructInit { name, fields };
                continue;
            }

            if self.match_kind(&TokenKind::Dot) {
                let field = match &self.advance().kind {
                    TokenKind::Identifier(name) => name.clone(),
                    _ => {
                        return Err(RainbowError::new(
                            "expected a field name after '.'",
                            self.previous().span,
                        ));
                    }
                };
                expr = Expr::Field {
                    object: Box::new(expr),
                    field,
                };
                continue;
            }

            if self.match_kind(&TokenKind::LBracket) {
                let index = self.expression()?;
                self.consume(&TokenKind::RBracket, "expected ']' after index")?;
                expr = Expr::Index {
                    collection: Box::new(expr),
                    index: Box::new(index),
                };
                continue;
            }

            break;
        }

        Ok(expr)
    }

    fn struct_initializer_fields(&mut self) -> RainbowResult<Vec<(String, Expr)>> {
        let mut fields = Vec::new();

        if self.check(&TokenKind::RBrace) {
            self.advance();
            return Ok(fields);
        }

        loop {
            let field_name = match &self.advance().kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(RainbowError::new(
                        "expected a field name in struct initializer",
                        self.previous().span,
                    ));
                }
            };

            self.consume(&TokenKind::Colon, "expected ':' after field name")?;
            let value = self.expression()?;
            fields.push((field_name, value));

            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }

        self.consume(&TokenKind::RBrace, "expected '}' after struct initializer")?;
        Ok(fields)
    }

    fn primary(&mut self) -> RainbowResult<Expr> {
        let token = self.advance();

        match &token.kind {
            TokenKind::Int(value) => Ok(Expr::Int(*value)),
            TokenKind::Float(value) => Ok(Expr::Float(*value)),
            TokenKind::Str(value) => Ok(Expr::Str(value.clone())),
            TokenKind::True => Ok(Expr::Bool(true)),
            TokenKind::False => Ok(Expr::Bool(false)),
            TokenKind::Nil => Ok(Expr::Nil),
            TokenKind::Identifier(name) => Ok(Expr::Variable(name.clone())),
            TokenKind::LBracket => self.array_literal(),
            TokenKind::LParen => {
                let expr = self.expression()?;
                self.consume(&TokenKind::RParen, "expected ')' after expression")?;
                Ok(expr)
            }
            _ => Err(RainbowError::new("expected an expression", token.span)),
        }
    }

    fn array_literal(&mut self) -> RainbowResult<Expr> {
        let mut elements = Vec::new();

        if self.check(&TokenKind::RBracket) {
            self.advance();
            return Ok(Expr::Array(elements));
        }

        loop {
            elements.push(self.expression()?);
            if !self.match_kind(&TokenKind::Comma) {
                break;
            }
        }

        self.consume(&TokenKind::RBracket, "expected ']' after array literal")?;
        Ok(Expr::Array(elements))
    }

    fn match_binary(&mut self, choices: &[(TokenKind, BinaryOp)]) -> Option<BinaryOp> {
        for (kind, op) in choices {
            if self.match_kind(kind) {
                return Some(*op);
            }
        }

        None
    }

    fn check_identifier_assignment(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Identifier(_))
            && self
                .tokens
                .get(self.current + 1)
                .is_some_and(|token| discriminant_eq(&token.kind, &TokenKind::Equal))
    }

    fn check_statement_boundary(&self) -> bool {
        self.check(&TokenKind::Newline) || self.check(&TokenKind::Dedent) || self.is_at_end()
    }

    fn consume_statement_separator(&mut self) -> RainbowResult<()> {
        if self.is_at_end() {
            return Ok(());
        }

        if self.previous_is(&TokenKind::Dedent) || self.check(&TokenKind::Dedent) {
            return Ok(());
        }

        if self.match_kind(&TokenKind::Newline) {
            self.skip_newlines();
            return Ok(());
        }

        Err(RainbowError::new(
            "expected a newline after statement",
            self.peek().span,
        ))
    }

    fn block(&mut self, label: &str) -> RainbowResult<Vec<Statement>> {
        self.consume(&TokenKind::Newline, "expected a newline before block")?;
        self.consume(&TokenKind::Indent, "expected an indented block")?;
        self.skip_newlines();

        let mut statements = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            statements.push(self.statement()?);
            self.consume_statement_separator()?;
        }

        if statements.is_empty() {
            return Err(RainbowError::new(
                format!("expected at least one statement in {label}"),
                self.peek().span,
            ));
        }

        self.consume(&TokenKind::Dedent, "expected the block to dedent")?;
        Ok(statements)
    }

    fn skip_newlines(&mut self) {
        while self.match_kind(&TokenKind::Newline) {}
    }

    fn consume(&mut self, kind: &TokenKind, message: &str) -> RainbowResult<()> {
        if self.match_kind(kind) {
            Ok(())
        } else {
            Err(RainbowError::new(message, self.peek().span))
        }
    }

    fn match_kind(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn check(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            return matches!(kind, TokenKind::Eof);
        }

        discriminant_eq(&self.peek().kind, kind)
    }

    fn previous_is(&self, kind: &TokenKind) -> bool {
        self.current > 0 && discriminant_eq(&self.previous().kind, kind)
    }

    fn advance(&mut self) -> &'a Token {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.previous()
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::Eof)
    }

    fn peek(&self) -> &'a Token {
        &self.tokens[self.current]
    }

    fn previous(&self) -> &'a Token {
        &self.tokens[self.current - 1]
    }
}

fn discriminant_eq(left: &TokenKind, right: &TokenKind) -> bool {
    std::mem::discriminant(left) == std::mem::discriminant(right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    #[test]
    fn parses_let_statement() {
        let tokens = lex("let speed = 3 * 14\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert_eq!(program.statements.len(), 1);
        assert!(matches!(program.statements[0], Statement::Let { .. }));
    }

    #[test]
    fn parses_import_statement() {
        let tokens = lex("import \"lib.rain\"\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert_eq!(
            program.statements[0],
            Statement::Import {
                path: "lib.rain".to_owned(),
                span: Span::new(1, 1),
                source_path: None,
            }
        );
    }

    #[test]
    fn preserves_operator_precedence() {
        let tokens = lex("1 + 2 * 3\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Expr {
            expr: Expr::Binary { op, .. },
            ..
        } = &program.statements[0]
        else {
            panic!("expected binary expression");
        };

        assert_eq!(*op, BinaryOp::Add);
    }

    #[test]
    fn parses_flow_expressions() {
        let tokens = lex("\"  Rainbow  \" then trim then lower\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Expr {
            expr:
                Expr::Flow {
                    value,
                    callee,
                    args,
                },
            ..
        } = &program.statements[0]
        else {
            panic!("expected flow expression");
        };

        assert_eq!(callee, "lower");
        assert!(args.is_empty());
        assert!(matches!(
            value.as_ref(),
            Expr::Flow {
                callee,
                ..
            } if callee == "trim"
        ));
    }

    #[test]
    fn parses_var_assignment_and_while() {
        let tokens = lex(r#"
var total = 0
var i = 1
while i <= 3:
    total = total + i
    i = i + 1
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(program.statements[0], Statement::Var { .. }));
        assert!(matches!(
            program.statements[2],
            Statement::While { ref body, .. } if matches!(body[0], Statement::Assign { .. })
        ));
    }

    #[test]
    fn parses_for_loops() {
        let tokens = lex(r#"
for value in [1, 2, 3]:
    print(value)
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::For { ref body, .. } if matches!(
                body[0],
                Statement::Expr {
                    expr: Expr::Call { .. },
                    ..
                }
            )
        ));
    }

    #[test]
    fn parses_statement_if_without_else() {
        let tokens = lex(r#"
if true:
    print("yes")
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::If {
                ref else_branch,
                ..
            } if else_branch.is_empty()
        ));
    }

    #[test]
    fn parses_statement_if_with_elif() {
        let tokens = lex(r#"
if value < 0:
    print("negative")
elif value == 0:
    print("zero")
else:
    print("positive")
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::If {
                else_branch: ref first_tail,
                ..
            } if matches!(
                first_tail.as_slice(),
                [Statement::If {
                    else_branch,
                    ..
                }] if !else_branch.is_empty()
            )
        ));
    }

    #[test]
    fn parses_return_break_and_continue() {
        let tokens = lex(r#"
fn scan(limit: i64) -> i64:
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
    return i
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Fn { body, .. } = &program.statements[0] else {
            panic!("expected function");
        };

        assert!(matches!(body.last(), Some(Statement::Return { .. })));
    }

    #[test]
    fn parses_structs_and_field_access() {
        let tokens = lex(r#"
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
p.x + p.y
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(program.statements[0], Statement::Struct { .. }));
        assert!(matches!(
            program.statements[2],
            Statement::Expr {
                expr: Expr::Binary { .. },
                ..
            }
        ));
    }

    #[test]
    fn parses_arrays_and_indexing() {
        let tokens = lex(r#"
fn first(items: [i64]) -> i64:
    return items[0]

let values = [3, 4, 5]
first(values)
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Fn { params, .. } = &program.statements[0] else {
            panic!("expected function");
        };

        assert!(matches!(params[0].ty, TypeName::Array(_)));
        assert!(matches!(program.statements[1], Statement::Let { .. }));
    }

    #[test]
    fn parses_annotated_bindings() {
        let tokens = lex(r#"
let count: i64 = 42
var values: [i64] = []
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Let {
                ty: TypeName::I64,
                ..
            }
        ));
        assert!(matches!(
            program.statements[1],
            Statement::Var {
                ty: TypeName::Array(_),
                ..
            }
        ));
    }

    #[test]
    fn parses_enum_declarations_and_variants() {
        let tokens = lex(r#"
enum Status:
    Pending
    Ready

let status: Status = Status.Ready
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Enum {
                ref name,
                ref variants,
                ..
            } if name == "Status"
                && variants
                    == &vec![
                        EnumVariant {
                            name: "Pending".to_owned(),
                            payload: None,
                            span: Span::new(3, 5),
                        },
                        EnumVariant {
                            name: "Ready".to_owned(),
                            payload: None,
                            span: Span::new(4, 5),
                        },
                    ]
        ));
        assert!(matches!(
            program.statements[1],
            Statement::Let {
                ty: TypeName::Struct(ref name),
                value: Expr::Field { .. },
                ..
            } if name == "Status"
        ));
    }

    #[test]
    fn parses_match_expressions() {
        let tokens = lex(r#"
let label = match status:
    Status.Pending:
        "pending"
    Status.Ready:
        "ready"
    else:
        "other"
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Let {
                value: Expr::Match { ref arms, .. },
                ..
            } if arms.len() == 3
                && matches!(
                    arms[0].pattern,
                    MatchPattern::Variant { ref enum_name, ref variant, binding: None }
                    if enum_name == "Status" && variant == "Pending"
                )
                && matches!(arms[2].pattern, MatchPattern::Else)
        ));
    }

    #[test]
    fn parses_payload_enum_variants_and_match_bindings() {
        let tokens = lex(r#"
enum Result:
    Ok(i64)
    Err(str)

let result = Result.Ok(42)
let value = match result:
    Result.Ok(inner):
        inner
    Result.Err(message):
        len(message)
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Enum {
                ref name,
                ref variants,
                ..
            } if name == "Result"
                && variants.len() == 2
                && variants[0].name == "Ok"
                && variants[0].payload.as_ref() == Some(&TypeName::I64)
                && variants[1].name == "Err"
                && variants[1].payload.as_ref() == Some(&TypeName::Str)
        ));
        assert!(matches!(
            program.statements[1],
            Statement::Let {
                value: Expr::EnumInit {
                    ref enum_name,
                    ref variant,
                    value: Some(_),
                },
                ..
            } if enum_name == "Result" && variant == "Ok"
        ));
        assert!(matches!(
            program.statements[2],
            Statement::Let {
                value: Expr::Match { ref arms, .. },
                ..
            } if matches!(
                arms[0].pattern,
                MatchPattern::Variant {
                    ref enum_name,
                    ref variant,
                    binding: Some(ref binding),
                } if enum_name == "Result" && variant == "Ok" && binding == "inner"
            )
        ));
    }

    #[test]
    fn parses_nil_and_nullable_types() {
        let tokens = lex(r#"
let maybe: i64? = nil
let rows: [str?]? = nil
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Let {
                ty: TypeName::Nullable(_),
                value: Expr::Nil,
                ..
            }
        ));
        assert!(matches!(
            program.statements[1],
            Statement::Let {
                ty: TypeName::Nullable(_),
                value: Expr::Nil,
                ..
            }
        ));
    }

    #[test]
    fn parses_if_let_statements_and_expressions() {
        let tokens = lex(r#"
let maybe: i64? = 42
if let value = maybe:
    print(value)
else:
    print(0)

let recovered = if let value = maybe:
    value
else:
    0

let result = Result.Ok(42)
if let Result.Ok(inner) = result:
    print(inner)
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[1],
            Statement::IfLet {
                pattern: IfLetPattern::Binding { ref name },
                value: Expr::Variable(_),
                ..
            } if name == "value"
        ));
        assert!(matches!(
            program.statements[2],
            Statement::Let {
                value: Expr::IfLet {
                    pattern: IfLetPattern::Binding { ref name },
                    ..
                },
                ..
            } if name == "value"
        ));
        assert!(matches!(
            program.statements[4],
            Statement::IfLet {
                pattern: IfLetPattern::Variant {
                    ref enum_name,
                    ref variant,
                    binding: Some(ref binding),
                },
                value: Expr::Variable(_),
                ..
            } if enum_name == "Result" && variant == "Ok" && binding == "inner"
        ));
    }

    #[test]
    fn parses_float_literals_and_annotations() {
        let tokens = lex("let ratio: f64 = 2.75\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Let {
                ty: TypeName::F64,
                value: Expr::Float(value),
                ..
            } if value == 2.75
        ));
    }

    #[test]
    fn parses_coalesce_operator_right_associative() {
        let tokens = lex("let value = first ?? second ?? 0\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Let {
            value: Expr::Binary { op, right, .. },
            ..
        } = &program.statements[0]
        else {
            panic!("expected coalesce binding");
        };

        assert_eq!(*op, BinaryOp::Coalesce);
        assert!(matches!(
            right.as_ref(),
            Expr::Binary {
                op: BinaryOp::Coalesce,
                ..
            }
        ));
    }

    #[test]
    fn parses_functions_with_if_statement_value() {
        let tokens = lex(r#"
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Fn {
            name,
            params,
            return_type,
            body,
            ..
        } = &program.statements[0]
        else {
            panic!("expected function statement");
        };

        assert_eq!(name, "fib");
        assert_eq!(*return_type, TypeName::I64);
        assert_eq!(
            params,
            &vec![Param {
                name: "n".to_owned(),
                ty: TypeName::I64,
            }]
        );
        assert!(matches!(
            body[0],
            Statement::If {
                ref else_branch,
                ..
            } if !else_branch.is_empty()
        ));
    }

    #[test]
    fn parses_if_expression_in_binding() {
        let tokens = lex(r#"
let value = if true:
    1
else:
    2
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Let {
                value: Expr::If { .. },
                ..
            }
        ));
    }

    #[test]
    fn parses_if_expression_with_elif() {
        let tokens = lex(r#"
let label = if value < 0:
    "negative"
elif value == 0:
    "zero"
else:
    "positive"
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        assert!(matches!(
            program.statements[0],
            Statement::Let {
                value: Expr::If {
                    else_branch: ref first_tail,
                    ..
                },
                ..
            } if matches!(
                first_tail.as_slice(),
                [Statement::If {
                    else_branch,
                    ..
                }] if !else_branch.is_empty()
            )
        ));
    }

    #[test]
    fn rejects_if_expression_elif_without_final_else() {
        let tokens = lex(r#"
let label = if value < 0:
    "negative"
elif value == 0:
    "zero"
"#)
        .expect("lexing should pass");
        let error = parse(&tokens).expect_err("if expression should require final else");

        assert!(error.message.contains("expected 'else' branch"));
    }

    #[test]
    fn parses_functions_without_type_annotations() {
        let tokens = lex(r#"
fn fib(n):
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)
"#)
        .expect("lexing should pass");
        let program = parse(&tokens).expect("untyped function should parse");

        let Statement::Fn {
            params,
            return_type,
            ..
        } = &program.statements[0]
        else {
            panic!("expected function statement");
        };

        assert_eq!(*return_type, TypeName::Infer);
        assert_eq!(
            params,
            &vec![Param {
                name: "n".to_owned(),
                ty: TypeName::Infer,
            }]
        );
    }
}

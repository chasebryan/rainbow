use crate::ast::{BinaryOp, Expr, Param, Program, Statement, TypeName, UnaryOp};
use crate::diagnostic::{FyrError, FyrResult};
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: &[Token]) -> FyrResult<Program> {
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

    fn parse(mut self) -> FyrResult<Program> {
        let mut statements = Vec::new();

        self.skip_newlines();
        while !self.is_at_end() {
            statements.push(self.statement()?);
            self.consume_statement_separator()?;
        }

        Ok(Program { statements })
    }

    fn statement(&mut self) -> FyrResult<Statement> {
        if self.match_kind(&TokenKind::Let) {
            return self.let_statement();
        }

        if self.match_kind(&TokenKind::Var) {
            return self.var_statement();
        }

        if self.match_kind(&TokenKind::Fn) {
            return self.fn_statement();
        }

        if self.match_kind(&TokenKind::Struct) {
            return self.struct_statement();
        }

        if self.match_kind(&TokenKind::While) {
            return self.while_statement();
        }

        if self.match_kind(&TokenKind::For) {
            return self.for_statement();
        }

        if self.match_kind(&TokenKind::If) {
            return self.if_statement();
        }

        if self.match_kind(&TokenKind::Return) {
            return self.return_statement();
        }

        if self.match_kind(&TokenKind::Break) {
            return Ok(Statement::Break);
        }

        if self.match_kind(&TokenKind::Continue) {
            return Ok(Statement::Continue);
        }

        if self.check_identifier_assignment() {
            return self.assignment_statement();
        }

        Ok(Statement::Expr(self.expression()?))
    }

    fn let_statement(&mut self) -> FyrResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(FyrError::new(
                    "expected an identifier after 'let'",
                    self.previous().span,
                ));
            }
        };

        let ty = self.optional_type_annotation()?;
        self.consume(&TokenKind::Equal, "expected '=' after binding name")?;
        let value = self.expression()?;

        Ok(Statement::Let { name, ty, value })
    }

    fn var_statement(&mut self) -> FyrResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(FyrError::new(
                    "expected an identifier after 'var'",
                    self.previous().span,
                ));
            }
        };

        let ty = self.optional_type_annotation()?;
        self.consume(&TokenKind::Equal, "expected '=' after mutable binding name")?;
        let value = self.expression()?;

        Ok(Statement::Var { name, ty, value })
    }

    fn assignment_statement(&mut self) -> FyrResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => unreachable!("caller checks assignment shape"),
        };

        self.consume(&TokenKind::Equal, "expected '=' in assignment")?;
        let value = self.expression()?;

        Ok(Statement::Assign { name, value })
    }

    fn fn_statement(&mut self) -> FyrResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(FyrError::new(
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
        })
    }

    fn struct_statement(&mut self) -> FyrResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(FyrError::new(
                    "expected a struct name after 'struct'",
                    self.previous().span,
                ));
            }
        };

        self.consume(&TokenKind::Colon, "expected ':' before struct fields")?;
        let fields = self.struct_fields()?;

        Ok(Statement::Struct { name, fields })
    }

    fn struct_fields(&mut self) -> FyrResult<Vec<Param>> {
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
                    return Err(FyrError::new(
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
            return Err(FyrError::new(
                "expected at least one field in struct",
                self.peek().span,
            ));
        }

        self.consume(&TokenKind::Dedent, "expected struct fields to dedent")?;
        Ok(fields)
    }

    fn while_statement(&mut self) -> FyrResult<Statement> {
        let condition = self.or()?;
        self.consume(&TokenKind::Colon, "expected ':' after while condition")?;
        let body = self.block("while body")?;

        Ok(Statement::While { condition, body })
    }

    fn for_statement(&mut self) -> FyrResult<Statement> {
        let name = match &self.advance().kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => {
                return Err(FyrError::new(
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
        })
    }

    fn if_statement(&mut self) -> FyrResult<Statement> {
        let condition = self.or()?;
        self.consume(&TokenKind::Colon, "expected ':' after if condition")?;
        let then_branch = self.block("if body")?;

        self.skip_newlines();
        let else_branch = if self.match_kind(&TokenKind::Else) {
            self.consume(&TokenKind::Colon, "expected ':' after else")?;
            self.block("else body")?
        } else {
            Vec::new()
        };

        Ok(Statement::If {
            condition,
            then_branch,
            else_branch,
        })
    }

    fn return_statement(&mut self) -> FyrResult<Statement> {
        let value = if self.check_statement_boundary() {
            None
        } else {
            Some(self.expression()?)
        };

        Ok(Statement::Return { value })
    }

    fn parameter_list(&mut self) -> FyrResult<Vec<Param>> {
        let mut params = Vec::new();

        if self.check(&TokenKind::RParen) {
            return Ok(params);
        }

        loop {
            let token = self.advance();
            let name = match &token.kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(FyrError::new(
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

    fn optional_type_annotation(&mut self) -> FyrResult<TypeName> {
        if self.match_kind(&TokenKind::Colon) {
            self.type_name()
        } else {
            Ok(TypeName::Infer)
        }
    }

    fn type_name(&mut self) -> FyrResult<TypeName> {
        let token = self.advance();

        match &token.kind {
            TokenKind::LBracket => {
                let element = self.type_name()?;
                self.consume(&TokenKind::RBracket, "expected ']' after array type")?;
                Ok(TypeName::Array(Box::new(element)))
            }
            TokenKind::Identifier(name) if name == "i64" => Ok(TypeName::I64),
            TokenKind::Identifier(name) if name == "bool" => Ok(TypeName::Bool),
            TokenKind::Identifier(name) if name == "str" => Ok(TypeName::Str),
            TokenKind::Identifier(name) if name == "unit" => Ok(TypeName::Unit),
            TokenKind::Identifier(name) => Ok(TypeName::Struct(name.clone())),
            _ => Err(FyrError::new("expected a type name", token.span)),
        }
    }

    fn expression(&mut self) -> FyrResult<Expr> {
        if self.match_kind(&TokenKind::If) {
            return self.if_expression();
        }

        self.or()
    }

    fn if_expression(&mut self) -> FyrResult<Expr> {
        let condition = self.or()?;
        self.consume(&TokenKind::Colon, "expected ':' after if condition")?;
        let then_branch = self.block("if body")?;

        self.skip_newlines();
        self.consume(&TokenKind::Else, "expected 'else' branch for if expression")?;
        self.consume(&TokenKind::Colon, "expected ':' after else")?;
        let else_branch = self.block("else body")?;

        Ok(Expr::If {
            condition: Box::new(condition),
            then_branch,
            else_branch,
        })
    }

    fn or(&mut self) -> FyrResult<Expr> {
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

    fn and(&mut self) -> FyrResult<Expr> {
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

    fn equality(&mut self) -> FyrResult<Expr> {
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

    fn comparison(&mut self) -> FyrResult<Expr> {
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

    fn term(&mut self) -> FyrResult<Expr> {
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

    fn factor(&mut self) -> FyrResult<Expr> {
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

    fn unary(&mut self) -> FyrResult<Expr> {
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

    fn call(&mut self) -> FyrResult<Expr> {
        let mut expr = self.primary()?;

        loop {
            if self.match_kind(&TokenKind::LParen) {
                let Expr::Variable(callee) = expr else {
                    return Err(FyrError::new(
                        "only named functions can be called in Fyr bootstrap",
                        self.previous().span,
                    ));
                };

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
                expr = Expr::Call { callee, args };
                continue;
            }

            if self.match_kind(&TokenKind::LBrace) {
                let Expr::Variable(name) = expr else {
                    return Err(FyrError::new(
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
                        return Err(FyrError::new(
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

    fn struct_initializer_fields(&mut self) -> FyrResult<Vec<(String, Expr)>> {
        let mut fields = Vec::new();

        if self.check(&TokenKind::RBrace) {
            self.advance();
            return Ok(fields);
        }

        loop {
            let field_name = match &self.advance().kind {
                TokenKind::Identifier(name) => name.clone(),
                _ => {
                    return Err(FyrError::new(
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

    fn primary(&mut self) -> FyrResult<Expr> {
        let token = self.advance();

        match &token.kind {
            TokenKind::Int(value) => Ok(Expr::Int(*value)),
            TokenKind::Str(value) => Ok(Expr::Str(value.clone())),
            TokenKind::True => Ok(Expr::Bool(true)),
            TokenKind::False => Ok(Expr::Bool(false)),
            TokenKind::Identifier(name) => Ok(Expr::Variable(name.clone())),
            TokenKind::LBracket => self.array_literal(),
            TokenKind::LParen => {
                let expr = self.expression()?;
                self.consume(&TokenKind::RParen, "expected ')' after expression")?;
                Ok(expr)
            }
            _ => Err(FyrError::new("expected an expression", token.span)),
        }
    }

    fn array_literal(&mut self) -> FyrResult<Expr> {
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

    fn consume_statement_separator(&mut self) -> FyrResult<()> {
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

        Err(FyrError::new(
            "expected a newline after statement",
            self.peek().span,
        ))
    }

    fn block(&mut self, label: &str) -> FyrResult<Vec<Statement>> {
        self.consume(&TokenKind::Newline, "expected a newline before block")?;
        self.consume(&TokenKind::Indent, "expected an indented block")?;
        self.skip_newlines();

        let mut statements = Vec::new();
        while !self.check(&TokenKind::Dedent) && !self.is_at_end() {
            statements.push(self.statement()?);
            self.consume_statement_separator()?;
        }

        if statements.is_empty() {
            return Err(FyrError::new(
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

    fn consume(&mut self, kind: &TokenKind, message: &str) -> FyrResult<()> {
        if self.match_kind(kind) {
            Ok(())
        } else {
            Err(FyrError::new(message, self.peek().span))
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
    fn preserves_operator_precedence() {
        let tokens = lex("1 + 2 * 3\n").expect("lexing should pass");
        let program = parse(&tokens).expect("parsing should pass");

        let Statement::Expr(Expr::Binary { op, .. }) = &program.statements[0] else {
            panic!("expected binary expression");
        };

        assert_eq!(*op, BinaryOp::Add);
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
            Statement::For { ref body, .. } if matches!(body[0], Statement::Expr(Expr::Call { .. }))
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
            Statement::Expr(Expr::Binary { .. })
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

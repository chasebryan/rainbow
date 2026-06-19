use crate::diagnostic::{FyrError, FyrResult};
use crate::span::Span;

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Eof,
    Newline,
    Indent,
    Dedent,
    Identifier(String),
    Int(i64),
    Float(f64),
    Str(String),
    True,
    False,
    Nil,
    Let,
    Var,
    Fn,
    For,
    In,
    Import,
    If,
    Match,
    Elif,
    Else,
    While,
    Return,
    Break,
    Continue,
    Struct,
    Enum,
    Arrow,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Colon,
    Dot,
    QuestionQuestion,
    Question,
    Equal,
    EqualEqual,
    Bang,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    PipeForward,
}

pub fn lex(source: &str) -> FyrResult<Vec<Token>> {
    Lexer::new(source).lex()
}

struct Lexer {
    chars: Vec<char>,
    current: usize,
    line: usize,
    column: usize,
    tokens: Vec<Token>,
    indent_stack: Vec<usize>,
    at_line_start: bool,
}

impl Lexer {
    fn new(source: &str) -> Self {
        Self {
            chars: source.chars().collect(),
            current: 0,
            line: 1,
            column: 1,
            tokens: Vec::new(),
            indent_stack: vec![0],
            at_line_start: true,
        }
    }

    fn lex(mut self) -> FyrResult<Vec<Token>> {
        while !self.is_at_end() {
            if self.at_line_start {
                self.handle_line_start()?;
                if self.at_line_start {
                    continue;
                }
            }

            let span = self.span();
            let ch = self.advance();

            match ch {
                ' ' | '\t' | '\r' => {}
                '\n' => self.tokens.push(Token {
                    kind: TokenKind::Newline,
                    span,
                }),
                '#' => self.skip_line_comment(),
                '/' if self.match_char('/') => self.skip_line_comment(),
                '+' => self.simple(TokenKind::Plus, span),
                '-' if self.match_char('>') => self.simple(TokenKind::Arrow, span),
                '-' => self.simple(TokenKind::Minus, span),
                '*' => self.simple(TokenKind::Star, span),
                '/' => self.simple(TokenKind::Slash, span),
                '%' => self.simple(TokenKind::Percent, span),
                '(' => self.simple(TokenKind::LParen, span),
                ')' => self.simple(TokenKind::RParen, span),
                '{' => self.simple(TokenKind::LBrace, span),
                '}' => self.simple(TokenKind::RBrace, span),
                '[' => self.simple(TokenKind::LBracket, span),
                ']' => self.simple(TokenKind::RBracket, span),
                ',' => self.simple(TokenKind::Comma, span),
                ':' => self.simple(TokenKind::Colon, span),
                '.' => self.simple(TokenKind::Dot, span),
                '?' if self.match_char('?') => self.simple(TokenKind::QuestionQuestion, span),
                '?' => self.simple(TokenKind::Question, span),
                '=' if self.match_char('=') => self.simple(TokenKind::EqualEqual, span),
                '=' => self.simple(TokenKind::Equal, span),
                '!' if self.match_char('=') => self.simple(TokenKind::BangEqual, span),
                '!' => self.simple(TokenKind::Bang, span),
                '<' if self.match_char('=') => self.simple(TokenKind::LessEqual, span),
                '<' => self.simple(TokenKind::Less, span),
                '>' if self.match_char('=') => self.simple(TokenKind::GreaterEqual, span),
                '>' => self.simple(TokenKind::Greater, span),
                '&' if self.match_char('&') => self.simple(TokenKind::AndAnd, span),
                '|' if self.match_char('|') => self.simple(TokenKind::OrOr, span),
                '|' if self.match_char('>') => self.simple(TokenKind::PipeForward, span),
                '"' => self.string(span)?,
                ch if ch.is_ascii_digit() => self.number(ch, span)?,
                ch if is_identifier_start(ch) => self.identifier(ch, span),
                ch => {
                    return Err(FyrError::new(format!("unexpected character '{ch}'"), span));
                }
            }

            if ch == '\n' {
                self.at_line_start = true;
            }
        }

        while self.indent_stack.len() > 1 {
            self.indent_stack.pop();
            self.tokens.push(Token {
                kind: TokenKind::Dedent,
                span: self.span(),
            });
        }

        self.tokens.push(Token {
            kind: TokenKind::Eof,
            span: self.span(),
        });
        Ok(self.tokens)
    }

    fn is_at_end(&self) -> bool {
        self.current >= self.chars.len()
    }

    fn span(&self) -> Span {
        Span::new(self.line, self.column)
    }

    fn advance(&mut self) -> char {
        let ch = self.chars[self.current];
        self.current += 1;

        if ch == '\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }

        ch
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.current).copied()
    }

    fn peek_next(&self) -> Option<char> {
        self.chars.get(self.current + 1).copied()
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn simple(&mut self, kind: TokenKind, span: Span) {
        self.tokens.push(Token { kind, span });
    }

    fn handle_line_start(&mut self) -> FyrResult<()> {
        let line = self.line;
        let mut indent = 0;

        while let Some(ch) = self.peek() {
            match ch {
                ' ' => {
                    indent += 1;
                    self.advance();
                }
                '\t' => {
                    indent += 4;
                    self.advance();
                }
                '\r' => {
                    self.advance();
                }
                _ => break,
            }
        }

        match self.peek() {
            None => return Ok(()),
            Some('\n') => {
                self.advance();
                return Ok(());
            }
            Some('#') => {
                self.skip_line_comment();
                if self.peek() == Some('\n') {
                    self.advance();
                }
                return Ok(());
            }
            _ => {}
        }

        let span = Span::new(line, self.column);
        let current_indent = *self
            .indent_stack
            .last()
            .expect("indent stack is never empty");

        if indent > current_indent {
            self.indent_stack.push(indent);
            self.tokens.push(Token {
                kind: TokenKind::Indent,
                span,
            });
        } else {
            while indent
                < *self
                    .indent_stack
                    .last()
                    .expect("indent stack is never empty")
            {
                self.indent_stack.pop();
                self.tokens.push(Token {
                    kind: TokenKind::Dedent,
                    span,
                });
            }

            if indent
                != *self
                    .indent_stack
                    .last()
                    .expect("indent stack is never empty")
            {
                return Err(FyrError::new("inconsistent indentation", span));
            }
        }

        self.at_line_start = false;
        Ok(())
    }

    fn skip_line_comment(&mut self) {
        while let Some(ch) = self.peek() {
            if ch == '\n' {
                break;
            }
            self.advance();
        }
    }

    fn string(&mut self, start: Span) -> FyrResult<()> {
        let mut value = String::new();

        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    self.advance();
                    self.tokens.push(Token {
                        kind: TokenKind::Str(value),
                        span: start,
                    });
                    return Ok(());
                }
                '\\' => {
                    self.advance();
                    value.push(self.escape_sequence(start)?);
                }
                '\n' => {
                    return Err(FyrError::new("unterminated string literal", start));
                }
                ch => {
                    self.advance();
                    value.push(ch);
                }
            }
        }

        Err(FyrError::new("unterminated string literal", start))
    }

    fn escape_sequence(&mut self, start: Span) -> FyrResult<char> {
        let Some(ch) = self.peek() else {
            return Err(FyrError::new("unterminated escape sequence", start));
        };
        self.advance();

        match ch {
            'n' => Ok('\n'),
            'r' => Ok('\r'),
            't' => Ok('\t'),
            '"' => Ok('"'),
            '\\' => Ok('\\'),
            other => Err(FyrError::new(
                format!("unknown escape sequence '\\{other}'"),
                start,
            )),
        }
    }

    fn number(&mut self, first: char, start: Span) -> FyrResult<()> {
        let mut raw = String::from(first);

        while let Some(ch) = self.peek() {
            if !ch.is_ascii_digit() {
                break;
            }
            self.advance();
            raw.push(ch);
        }

        if self.peek() == Some('.') && self.peek_next().is_some_and(|ch| ch.is_ascii_digit()) {
            self.advance();
            raw.push('.');

            while let Some(ch) = self.peek() {
                if !ch.is_ascii_digit() {
                    break;
                }
                self.advance();
                raw.push(ch);
            }

            let value = raw
                .parse::<f64>()
                .map_err(|_| FyrError::new("float literal is invalid", start))?;
            if !value.is_finite() {
                return Err(FyrError::new("float literal must be finite", start));
            }

            self.tokens.push(Token {
                kind: TokenKind::Float(value),
                span: start,
            });
            return Ok(());
        }

        let value = raw
            .parse::<i64>()
            .map_err(|_| FyrError::new("integer literal is too large", start))?;

        self.tokens.push(Token {
            kind: TokenKind::Int(value),
            span: start,
        });
        Ok(())
    }

    fn identifier(&mut self, first: char, start: Span) {
        let mut raw = String::from(first);

        while let Some(ch) = self.peek() {
            if !is_identifier_continue(ch) {
                break;
            }
            self.advance();
            raw.push(ch);
        }

        let kind = match raw.as_str() {
            "and" => TokenKind::AndAnd,
            "elif" => TokenKind::Elif,
            "else" => TokenKind::Else,
            "enum" => TokenKind::Enum,
            "break" => TokenKind::Break,
            "continue" => TokenKind::Continue,
            "false" => TokenKind::False,
            "fn" => TokenKind::Fn,
            "for" => TokenKind::For,
            "if" => TokenKind::If,
            "import" => TokenKind::Import,
            "in" => TokenKind::In,
            "let" => TokenKind::Let,
            "match" => TokenKind::Match,
            "nil" => TokenKind::Nil,
            "not" => TokenKind::Bang,
            "or" => TokenKind::OrOr,
            "return" => TokenKind::Return,
            "struct" => TokenKind::Struct,
            "true" => TokenKind::True,
            "var" => TokenKind::Var,
            "while" => TokenKind::While,
            _ => TokenKind::Identifier(raw),
        };

        self.tokens.push(Token { kind, span: start });
    }
}

fn is_identifier_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_identifier_continue(ch: char) -> bool {
    is_identifier_start(ch) || ch.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexes_keywords_and_operators() {
        let tokens = lex(
            "fn ok(value: bool) -> bool:\n    let ready = value && false\n    ready |> print\n",
        )
        .expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Fn,
                TokenKind::Identifier("ok".to_owned()),
                TokenKind::LParen,
                TokenKind::Identifier("value".to_owned()),
                TokenKind::Colon,
                TokenKind::Identifier("bool".to_owned()),
                TokenKind::RParen,
                TokenKind::Arrow,
                TokenKind::Identifier("bool".to_owned()),
                TokenKind::Colon,
                TokenKind::Newline,
                TokenKind::Indent,
                TokenKind::Let,
                TokenKind::Identifier("ready".to_owned()),
                TokenKind::Equal,
                TokenKind::Identifier("value".to_owned()),
                TokenKind::AndAnd,
                TokenKind::False,
                TokenKind::Newline,
                TokenKind::Identifier("ready".to_owned()),
                TokenKind::PipeForward,
                TokenKind::Identifier("print".to_owned()),
                TokenKind::Newline,
                TokenKind::Dedent,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_for_in_keywords() {
        let tokens = lex("for value in values:\n    print(value)\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert!(matches!(kinds[0], TokenKind::For));
        assert!(matches!(kinds[2], TokenKind::In));
    }

    #[test]
    fn lexes_elif_keyword() {
        let tokens = lex("elif ready:\n    print(\"yes\")\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert!(matches!(kinds[0], TokenKind::Elif));
    }

    #[test]
    fn lexes_import_keyword() {
        let tokens = lex("import \"lib.fyr\"\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Import,
                TokenKind::Str("lib.fyr".to_owned()),
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_enum_keyword() {
        let tokens = lex("enum Status:\n    Ready\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert!(matches!(kinds[0], TokenKind::Enum));
        assert!(matches!(kinds[1], TokenKind::Identifier(ref name) if name == "Status"));
        assert!(matches!(kinds[5], TokenKind::Identifier(ref name) if name == "Ready"));
    }

    #[test]
    fn lexes_match_keyword() {
        let tokens = lex("match status:\n    else:\n        0\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert!(matches!(kinds[0], TokenKind::Match));
        assert!(matches!(kinds[5], TokenKind::Else));
    }

    #[test]
    fn lexes_word_boolean_operators() {
        let tokens = lex("let ready = not false and true or false\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Let,
                TokenKind::Identifier("ready".to_owned()),
                TokenKind::Equal,
                TokenKind::Bang,
                TokenKind::False,
                TokenKind::AndAnd,
                TokenKind::True,
                TokenKind::OrOr,
                TokenKind::False,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_nil_nullable_marker_and_coalesce_operator() {
        let tokens = lex("let value: i64? = maybe ?? nil\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert_eq!(
            kinds,
            vec![
                TokenKind::Let,
                TokenKind::Identifier("value".to_owned()),
                TokenKind::Colon,
                TokenKind::Identifier("i64".to_owned()),
                TokenKind::Question,
                TokenKind::Equal,
                TokenKind::Identifier("maybe".to_owned()),
                TokenKind::QuestionQuestion,
                TokenKind::Nil,
                TokenKind::Newline,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_float_literals_without_breaking_field_dots() {
        let tokens = lex("let ratio = 2.75\nvalue.field\n").expect("lexing should pass");
        let kinds: Vec<TokenKind> = tokens.into_iter().map(|token| token.kind).collect();

        assert!(matches!(kinds[3], TokenKind::Float(value) if value == 2.75));
        assert!(matches!(kinds[5], TokenKind::Identifier(ref name) if name == "value"));
        assert!(matches!(kinds[6], TokenKind::Dot));
        assert!(matches!(kinds[7], TokenKind::Identifier(ref name) if name == "field"));
    }

    #[test]
    fn rejects_unterminated_strings() {
        let error = lex("\"fyr").expect_err("unterminated string should fail");

        assert!(error.message.contains("unterminated"));
    }
}

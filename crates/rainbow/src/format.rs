use crate::ast::{
    BinaryOp, Expr, IfLetPattern, MatchPattern, Param, Program, Statement, TypeName, UnaryOp,
};
use crate::diagnostic::RainbowResult;
use crate::lexer;
use crate::parser;

const INDENT: &str = "    ";

pub fn format_source(source: &str) -> RainbowResult<String> {
    let comments = CommentPlan::from_source(source);
    let tokens = lexer::lex(source)?;
    let program = parser::parse(&tokens)?;
    Ok(comments.apply(format_program(&program)))
}

pub fn format_program(program: &Program) -> String {
    let mut formatter = Formatter::new();
    formatter.statements(&program.statements, 0);
    formatter.output.push('\n');
    formatter.output
}

struct Formatter {
    output: String,
}

impl Formatter {
    fn new() -> Self {
        Self {
            output: String::new(),
        }
    }

    fn statements(&mut self, statements: &[Statement], indent: usize) {
        for (index, statement) in statements.iter().enumerate() {
            if index > 0 {
                self.output.push('\n');
            }
            self.statement(statement, indent);
        }
    }

    fn statement(&mut self, statement: &Statement, indent: usize) {
        self.write_indent(indent);
        match statement {
            Statement::Let {
                name, ty, value, ..
            } => {
                self.output.push_str("let ");
                self.output.push_str(name);
                self.optional_type(ty);
                self.output.push_str(" = ");
                self.expr(value, 0, indent);
            }
            Statement::Var {
                name, ty, value, ..
            } => {
                self.output.push_str("var ");
                self.output.push_str(name);
                self.optional_type(ty);
                self.output.push_str(" = ");
                self.expr(value, 0, indent);
            }
            Statement::Assign { name, value, .. } => {
                self.output.push_str(name);
                self.output.push_str(" = ");
                self.expr(value, 0, indent);
            }
            Statement::Import { path, .. } => {
                self.output.push_str("import ");
                self.string(path);
            }
            Statement::Struct { name, fields, .. } => {
                self.output.push_str("struct ");
                self.output.push_str(name);
                self.output.push_str(":\n");
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        self.output.push('\n');
                    }
                    self.write_indent(indent + 1);
                    self.output.push_str(&field.name);
                    self.output.push_str(": ");
                    self.output.push_str(&type_name(&field.ty));
                }
            }
            Statement::Enum { name, variants, .. } => {
                self.output.push_str("enum ");
                self.output.push_str(name);
                self.output.push_str(":\n");
                for (index, variant) in variants.iter().enumerate() {
                    if index > 0 {
                        self.output.push('\n');
                    }
                    self.write_indent(indent + 1);
                    self.output.push_str(&variant.name);
                    if let Some(payload) = &variant.payload {
                        self.output.push('(');
                        self.output.push_str(&type_name(payload));
                        self.output.push(')');
                    }
                }
            }
            Statement::Fn {
                name,
                params,
                return_type,
                body,
                ..
            } => {
                self.output.push_str("fn ");
                self.output.push_str(name);
                self.output.push('(');
                self.params(params);
                self.output.push(')');
                if *return_type != TypeName::Infer {
                    self.output.push_str(" -> ");
                    self.output.push_str(&type_name(return_type));
                }
                self.output.push_str(":\n");
                self.statements(body, indent + 1);
            }
            Statement::While {
                condition, body, ..
            } => {
                self.output.push_str("while ");
                self.expr(condition, 0, indent);
                self.output.push_str(":\n");
                self.statements(body, indent + 1);
            }
            Statement::For {
                name,
                iterable,
                body,
                ..
            } => {
                self.output.push_str("for ");
                self.output.push_str(name);
                self.output.push_str(" in ");
                self.expr(iterable, 0, indent);
                self.output.push_str(":\n");
                self.statements(body, indent + 1);
            }
            Statement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.if_statement(condition, then_branch, else_branch, indent, "if");
            }
            Statement::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
                ..
            } => {
                self.if_let_statement(pattern, value, then_branch, else_branch, indent, "if");
            }
            Statement::Return { value, .. } => {
                self.output.push_str("return");
                if let Some(value) = value {
                    self.output.push(' ');
                    self.expr(value, 0, indent);
                }
            }
            Statement::Break { .. } => self.output.push_str("break"),
            Statement::Continue { .. } => self.output.push_str("continue"),
            Statement::Expr { expr, .. } => self.expr(expr, 0, indent),
        }
    }

    fn if_statement(
        &mut self,
        condition: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
        indent: usize,
        keyword: &str,
    ) {
        self.output.push_str(keyword);
        self.output.push(' ');
        self.expr(condition, 0, indent);
        self.output.push_str(":\n");
        self.statements(then_branch, indent + 1);
        self.if_tail(else_branch, indent);
    }

    fn if_let_statement(
        &mut self,
        pattern: &IfLetPattern,
        value: &Expr,
        then_branch: &[Statement],
        else_branch: &[Statement],
        indent: usize,
        keyword: &str,
    ) {
        self.output.push_str(keyword);
        self.output.push_str(" let ");
        self.if_let_pattern(pattern);
        self.output.push_str(" = ");
        self.expr(value, 0, indent);
        self.output.push_str(":\n");
        self.statements(then_branch, indent + 1);
        self.if_tail(else_branch, indent);
    }

    fn if_tail(&mut self, else_branch: &[Statement], indent: usize) {
        match else_branch {
            [] => {}
            [
                Statement::If {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                },
            ] => {
                self.output.push('\n');
                self.write_indent(indent);
                self.if_statement(condition, then_branch, else_branch, indent, "elif");
            }
            [
                Statement::IfLet {
                    pattern,
                    value,
                    then_branch,
                    else_branch,
                    ..
                },
            ] => {
                self.output.push('\n');
                self.write_indent(indent);
                self.if_let_statement(pattern, value, then_branch, else_branch, indent, "elif");
            }
            statements => {
                self.output.push('\n');
                self.write_indent(indent);
                self.output.push_str("else:\n");
                self.statements(statements, indent + 1);
            }
        }
    }

    fn params(&mut self, params: &[Param]) {
        for (index, param) in params.iter().enumerate() {
            if index > 0 {
                self.output.push_str(", ");
            }
            self.output.push_str(&param.name);
            if param.ty != TypeName::Infer {
                self.output.push_str(": ");
                self.output.push_str(&type_name(&param.ty));
            }
        }
    }

    fn optional_type(&mut self, ty: &TypeName) {
        if *ty != TypeName::Infer {
            self.output.push_str(": ");
            self.output.push_str(&type_name(ty));
        }
    }

    fn expr(&mut self, expr: &Expr, parent_precedence: u8, indent: usize) {
        match expr {
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                self.output.push_str("if ");
                self.expr(condition, 0, indent);
                self.output.push_str(":\n");
                self.statements(then_branch, indent + 1);
                self.if_tail(else_branch, indent);
            }
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                self.output.push_str("if let ");
                self.if_let_pattern(pattern);
                self.output.push_str(" = ");
                self.expr(value, 0, indent);
                self.output.push_str(":\n");
                self.statements(then_branch, indent + 1);
                self.if_tail(else_branch, indent);
            }
            Expr::Match { value, arms } => {
                self.output.push_str("match ");
                self.expr(value, 0, indent);
                self.output.push_str(":\n");
                for (index, arm) in arms.iter().enumerate() {
                    if index > 0 {
                        self.output.push('\n');
                    }
                    self.write_indent(indent + 1);
                    self.match_pattern(&arm.pattern);
                    self.output.push_str(":\n");
                    self.statements(&arm.body, indent + 2);
                }
            }
            _ => self.expr_inline(expr, parent_precedence),
        }
    }

    fn if_let_pattern(&mut self, pattern: &IfLetPattern) {
        match pattern {
            IfLetPattern::Binding { name } => self.output.push_str(name),
            IfLetPattern::Variant {
                enum_name,
                variant,
                binding,
            } => {
                self.output.push_str(enum_name);
                self.output.push('.');
                self.output.push_str(variant);
                if let Some(binding) = binding {
                    self.output.push('(');
                    self.output.push_str(binding);
                    self.output.push(')');
                }
            }
        }
    }

    fn match_pattern(&mut self, pattern: &MatchPattern) {
        match pattern {
            MatchPattern::Variant {
                enum_name,
                variant,
                binding,
            } => {
                self.output.push_str(enum_name);
                self.output.push('.');
                self.output.push_str(variant);
                if let Some(binding) = binding {
                    self.output.push('(');
                    self.output.push_str(binding);
                    self.output.push(')');
                }
            }
            MatchPattern::Else => self.output.push_str("else"),
        }
    }

    fn expr_inline(&mut self, expr: &Expr, parent_precedence: u8) {
        let precedence = expr_precedence(expr);
        let needs_parens = precedence < parent_precedence;
        if needs_parens {
            self.output.push('(');
        }

        match expr {
            Expr::Int(value) => self.output.push_str(&value.to_string()),
            Expr::Float(value) => self.output.push_str(&format_float(*value)),
            Expr::Bool(value) => self.output.push_str(if *value { "true" } else { "false" }),
            Expr::Str(value) => self.string(value),
            Expr::Nil => self.output.push_str("nil"),
            Expr::Variable(name) => self.output.push_str(name),
            Expr::Unary { op, expr } => {
                self.output.push_str(match op {
                    UnaryOp::Negate => "-",
                    UnaryOp::Not => "not ",
                });
                self.expr_inline(
                    expr,
                    expr_precedence(&Expr::Unary {
                        op: *op,
                        expr: expr.clone(),
                    }),
                );
            }
            Expr::Binary { left, op, right } => {
                let precedence = binary_precedence(*op);
                self.expr_inline(left, precedence);
                self.output.push(' ');
                self.output.push_str(binary_op(*op));
                self.output.push(' ');
                self.expr_inline(right, precedence + 1);
            }
            Expr::Call { callee, args } => {
                self.output.push_str(callee);
                self.output.push('(');
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.expr(arg, 0, 0);
                }
                self.output.push(')');
            }
            Expr::Flow {
                value,
                callee,
                args,
            } => {
                self.expr_inline(value, expr_precedence(expr));
                self.output.push_str(" then ");
                self.output.push_str(callee);
                if !args.is_empty() {
                    self.output.push('(');
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            self.output.push_str(", ");
                        }
                        self.expr(arg, 0, 0);
                    }
                    self.output.push(')');
                }
            }
            Expr::StructInit { name, fields } => {
                self.output.push_str(name);
                self.output.push_str(" { ");
                for (index, (field, value)) in fields.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.output.push_str(field);
                    self.output.push_str(": ");
                    self.expr(value, 0, 0);
                }
                self.output.push_str(" }");
            }
            Expr::EnumInit {
                enum_name,
                variant,
                value,
            } => {
                self.output.push_str(enum_name);
                self.output.push('.');
                self.output.push_str(variant);
                self.output.push('(');
                if let Some(value) = value {
                    self.expr(value, 0, 0);
                }
                self.output.push(')');
            }
            Expr::Field { object, field } => {
                self.expr_inline(object, expr_precedence(expr));
                self.output.push('.');
                self.output.push_str(field);
            }
            Expr::Array(elements) => {
                self.output.push('[');
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        self.output.push_str(", ");
                    }
                    self.expr(element, 0, 0);
                }
                self.output.push(']');
            }
            Expr::Index { collection, index } => {
                self.expr_inline(collection, expr_precedence(expr));
                self.output.push('[');
                self.expr(index, 0, 0);
                self.output.push(']');
            }
            Expr::If { .. } | Expr::IfLet { .. } | Expr::Match { .. } => {
                unreachable!("if expressions are handled by expr")
            }
        }

        if needs_parens {
            self.output.push(')');
        }
    }

    fn string(&mut self, value: &str) {
        self.output.push('"');
        for ch in value.chars() {
            match ch {
                '\n' => self.output.push_str("\\n"),
                '\r' => self.output.push_str("\\r"),
                '\t' => self.output.push_str("\\t"),
                '"' => self.output.push_str("\\\""),
                '\\' => self.output.push_str("\\\\"),
                ch => self.output.push(ch),
            }
        }
        self.output.push('"');
    }

    fn write_indent(&mut self, indent: usize) {
        for _ in 0..indent {
            self.output.push_str(INDENT);
        }
    }
}

fn type_name(ty: &TypeName) -> String {
    match ty {
        TypeName::Infer => "infer".to_owned(),
        TypeName::I64 => "i64".to_owned(),
        TypeName::F64 => "f64".to_owned(),
        TypeName::Bool => "bool".to_owned(),
        TypeName::Str => "str".to_owned(),
        TypeName::Unit => "unit".to_owned(),
        TypeName::Struct(name) => name.clone(),
        TypeName::Array(element) => format!("[{}]", type_name(element)),
        TypeName::Nullable(inner) => format!("{}?", type_name(inner)),
    }
}

fn expr_precedence(expr: &Expr) -> u8 {
    match expr {
        Expr::If { .. } | Expr::IfLet { .. } | Expr::Match { .. } => 0,
        Expr::Flow { .. } => 2,
        Expr::Binary { op, .. } => binary_precedence(*op),
        Expr::Unary { .. } => 8,
        Expr::Call { .. }
        | Expr::StructInit { .. }
        | Expr::EnumInit { .. }
        | Expr::Field { .. }
        | Expr::Index { .. } => 9,
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Str(_)
        | Expr::Nil
        | Expr::Variable(_)
        | Expr::Array(_) => 10,
    }
}

fn binary_precedence(op: BinaryOp) -> u8 {
    match op {
        BinaryOp::Coalesce => 1,
        BinaryOp::Or => 3,
        BinaryOp::And => 4,
        BinaryOp::Equal | BinaryOp::NotEqual => 5,
        BinaryOp::Less | BinaryOp::LessEqual | BinaryOp::Greater | BinaryOp::GreaterEqual => 6,
        BinaryOp::Add | BinaryOp::Subtract => 7,
        BinaryOp::Multiply | BinaryOp::Divide | BinaryOp::Remainder => 8,
    }
}

fn binary_op(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "+",
        BinaryOp::Subtract => "-",
        BinaryOp::Multiply => "*",
        BinaryOp::Divide => "/",
        BinaryOp::Remainder => "%",
        BinaryOp::Equal => "==",
        BinaryOp::NotEqual => "!=",
        BinaryOp::Less => "<",
        BinaryOp::LessEqual => "<=",
        BinaryOp::Greater => ">",
        BinaryOp::GreaterEqual => ">=",
        BinaryOp::Coalesce => "??",
        BinaryOp::And => "and",
        BinaryOp::Or => "or",
    }
}

fn format_float(value: f64) -> String {
    let mut raw = value.to_string();
    if !raw.contains('.') && !raw.contains('e') && !raw.contains('E') {
        raw.push_str(".0");
    }
    raw
}

#[derive(Debug, Clone, Default)]
struct CommentPlan {
    leading: Vec<(String, Vec<String>)>,
    trailing: Vec<(String, String)>,
    eof: Vec<String>,
}

impl CommentPlan {
    fn from_source(source: &str) -> Self {
        let mut plan = Self::default();
        let mut pending = Vec::new();

        for line in source.lines() {
            let (code, comment) = split_code_comment(line);
            let code_key = compact_code_key(code);

            if code_key.is_empty() {
                if let Some(comment) = comment {
                    pending.push(clean_comment(comment));
                }
                continue;
            }

            if !pending.is_empty() {
                plan.leading
                    .push((code_key.clone(), std::mem::take(&mut pending)));
            }

            if let Some(comment) = comment {
                plan.trailing.push((code_key, clean_comment(comment)));
            }
        }

        plan.eof = pending;
        plan
    }

    fn apply(mut self, formatted: String) -> String {
        let mut output = String::new();

        for line in formatted.lines() {
            let key = compact_code_key(line);
            let indent = leading_whitespace(line);

            if !key.is_empty()
                && let Some(comments) = take_leading(&mut self.leading, &key)
            {
                for comment in comments {
                    output.push_str(indent);
                    output.push_str(&comment);
                    output.push('\n');
                }
            }

            output.push_str(line);

            if !key.is_empty()
                && let Some(comment) = take_trailing(&mut self.trailing, &key)
            {
                output.push_str("  ");
                output.push_str(&comment);
            }

            output.push('\n');
        }

        self.append_unmatched(&mut output);
        output
    }

    fn append_unmatched(self, output: &mut String) {
        for (_, comments) in self.leading {
            for comment in comments {
                output.push_str(&comment);
                output.push('\n');
            }
        }

        for (_, comment) in self.trailing {
            output.push_str(&comment);
            output.push('\n');
        }

        for comment in self.eof {
            output.push_str(&comment);
            output.push('\n');
        }
    }
}

fn line_comment_start(line: &str) -> Option<usize> {
    let mut chars = line.char_indices().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some((index, ch)) = chars.next() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '#' => return Some(index),
            '/' if chars.peek().is_some_and(|(_, next)| *next == '/') => return Some(index),
            _ => {}
        }
    }

    None
}

fn split_code_comment(line: &str) -> (&str, Option<&str>) {
    match line_comment_start(line) {
        Some(index) => (&line[..index], Some(&line[index..])),
        None => (line, None),
    }
}

fn clean_comment(comment: &str) -> String {
    comment.trim_start().to_owned()
}

fn take_leading(comments: &mut Vec<(String, Vec<String>)>, key: &str) -> Option<Vec<String>> {
    comments
        .iter()
        .position(|(candidate, _)| candidate == key)
        .map(|index| comments.remove(index).1)
}

fn take_trailing(comments: &mut Vec<(String, String)>, key: &str) -> Option<String> {
    comments
        .iter()
        .position(|(candidate, _)| candidate == key)
        .map(|index| comments.remove(index).1)
}

fn leading_whitespace(line: &str) -> &str {
    let end = line
        .char_indices()
        .find(|(_, ch)| !ch.is_whitespace())
        .map(|(index, _)| index)
        .unwrap_or(line.len());
    &line[..end]
}

fn compact_code_key(line: &str) -> String {
    let (code, _) = split_code_comment(line);
    let mut key = String::new();
    let mut in_string = false;
    let mut escaped = false;

    for ch in code.chars() {
        if in_string {
            key.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                key.push(ch);
            }
            ch if ch.is_whitespace() => {}
            ch => key.push(ch),
        }
    }

    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bindings_and_expressions() {
        let formatted = format_source(
            r#"
let   answer=40+2*3
var values:[i64]=[1,2,3]
print(values[0])
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "let answer = 40 + 2 * 3\nvar values: [i64] = [1, 2, 3]\nprint(values[0])\n"
        );
    }

    #[test]
    fn formats_imports() {
        let formatted = format_source("import   \"lib.rain\"\n").expect("formatting should pass");

        assert_eq!(formatted, "import \"lib.rain\"\n");
    }

    #[test]
    fn formats_enum_declarations() {
        let formatted = format_source(
            r#"
enum   Status:
  Pending
  Ready
let status:Status=Status.Ready
enum Result:
  Ok( i64 )
  Err(str)
let scored=Result.Ok(42)
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "enum Status:\n    Pending\n    Ready\nlet status: Status = Status.Ready\nenum Result:\n    Ok(i64)\n    Err(str)\nlet scored = Result.Ok(42)\n"
        );
    }

    #[test]
    fn formats_match_expressions() {
        let formatted = format_source(
            r#"
let label=match status:
  Status.Pending:
    "pending"
  else:
    "other"
let value=match result:
  Result.Ok(inner):
    inner
  Result.Err(message):
    len(message)
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "let label = match status:\n    Status.Pending:\n        \"pending\"\n    else:\n        \"other\"\nlet value = match result:\n    Result.Ok(inner):\n        inner\n    Result.Err(message):\n        len(message)\n"
        );
    }

    #[test]
    fn formats_nil_and_nullable_types() {
        let formatted = format_source(
            r#"
let maybe : i64 ? = nil
let values : [ i64 ? ] = [ nil , 1 ]
let recovered=maybe??10
let ratio:f64=3.140
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "let maybe: i64? = nil\nlet values: [i64?] = [nil, 1]\nlet recovered = maybe ?? 10\nlet ratio: f64 = 3.14\n"
        );
    }

    #[test]
    fn formats_flow_calls() {
        let formatted = format_source(
            r#"
let label="  Rainbow  " then trim then lower then replace("rainbow","Rainbow")
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "let label = \"  Rainbow  \" then trim then lower then replace(\"rainbow\", \"Rainbow\")\n"
        );
    }

    #[test]
    fn formats_blocks_and_elif_chains() {
        let formatted = format_source(
            r#"
fn label(value:i64)->str:
  if value<0:
    return "negative"
  elif value==0:
    return "zero"
  else:
    return "positive"
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "fn label(value: i64) -> str:\n    if value < 0:\n        return \"negative\"\n    elif value == 0:\n        return \"zero\"\n    else:\n        return \"positive\"\n"
        );
    }

    #[test]
    fn formats_if_let_chains() {
        let formatted = format_source(
            r#"
let maybe:i64?=42
let other:i64?=nil
if let value=maybe:
 print(value)
elif let fallback=other:
 print(fallback)
else:
 print(0)
if let Result.Ok(value)=result:
 print(value)
elif let Result.Err(message)=result:
 print(len(message))
"#,
        )
        .expect("formatting should pass");

        assert_eq!(
            formatted,
            "let maybe: i64? = 42\nlet other: i64? = nil\nif let value = maybe:\n    print(value)\nelif let fallback = other:\n    print(fallback)\nelse:\n    print(0)\nif let Result.Ok(value) = result:\n    print(value)\nelif let Result.Err(message) = result:\n    print(len(message))\n"
        );
    }

    #[test]
    fn preserves_parenthesized_right_associative_shape() {
        let formatted =
            format_source("let value = 10 - (3 - 1)\n").expect("formatting should pass");

        assert_eq!(formatted, "let value = 10 - (3 - 1)\n");
    }

    #[test]
    fn preserves_leading_and_trailing_comments() {
        let formatted = format_source(
            r#"
# Greeting setup.
let name="Rainbow" # keep the language name
print(name)
"#,
        )
        .expect("comments should be preserved");

        assert_eq!(
            formatted,
            "# Greeting setup.\nlet name = \"Rainbow\"  # keep the language name\nprint(name)\n"
        );
    }

    #[test]
    fn preserves_comments_inside_blocks() {
        let formatted = format_source(
            r#"
fn add(a:i64,b:i64)->i64:
  # Safe integer addition.
  return a+b # result
"#,
        )
        .expect("comments should be preserved");

        assert_eq!(
            formatted,
            "fn add(a: i64, b: i64) -> i64:\n    # Safe integer addition.\n    return a + b  # result\n"
        );
    }

    #[test]
    fn ignores_comment_markers_inside_strings() {
        let formatted = format_source("let tag = \"#not-comment\"\nprint(\"//also-text\")\n")
            .expect("comment markers inside strings should not be comments");

        assert_eq!(
            formatted,
            "let tag = \"#not-comment\"\nprint(\"//also-text\")\n"
        );
    }
}

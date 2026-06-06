pub mod ast;
pub mod diagnostic;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod repl;
pub mod span;
pub mod typecheck;

pub use diagnostic::{FyrError, FyrResult};
pub use eval::{RunResult, Value};

pub fn check_source(source: &str) -> FyrResult<()> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(&tokens)?;
    typecheck::check(&program)?;
    Ok(())
}

pub fn run_source(source: &str) -> FyrResult<RunResult> {
    let tokens = lexer::lex(source)?;
    let program = parser::parse(&tokens)?;
    typecheck::check(&program)?;
    eval::Evaluator::new().run(&program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_basic_program() {
        let source = r#"
let answer = 40 + 2
print(answer)
answer
"#;

        let result = run_source(source).expect("program should run");

        assert_eq!(result.outputs, vec!["42"]);
        assert_eq!(result.last_value, Value::Int(42));
    }

    #[test]
    fn check_rejects_invalid_syntax() {
        let error = check_source("let = 3").expect_err("syntax should fail");

        assert!(error.message.contains("identifier"));
    }

    #[test]
    fn check_rejects_type_errors() {
        let error = check_source(
            r#"
fn add(a: i64, b: i64) -> i64:
    a + b

add(1, false)
"#,
        )
        .expect_err("type error should fail");

        assert!(error.message.contains("expected i64"));
    }
}

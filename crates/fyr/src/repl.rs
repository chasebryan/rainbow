use std::fs;
use std::io::{self, Write};

use crate::ast::{Program, Statement};
use crate::diagnostic::FyrResult;
use crate::eval::{Evaluator, Value};
use crate::lexer;
use crate::parser;
use crate::typecheck;

pub struct ReplSession {
    evaluator: Evaluator,
    typecheck_history: String,
}

#[derive(Debug, PartialEq, Eq)]
enum ReplAction {
    Continue(Vec<String>),
    Exit,
}

pub fn start() -> FyrResult<()> {
    let mut session = ReplSession::new();
    let stdin = io::stdin();
    let mut buffer = String::new();
    let mut in_multiline = false;
    let mut saw_indented_line = false;

    println!("Rainbow 0.1.0 bootstrap REPL");
    println!("Type :help for commands, :quit to exit.");

    loop {
        if buffer.is_empty() {
            print!("fyr> ");
        } else {
            print!("... ");
        }
        io::stdout().flush().expect("stdout should flush");

        let mut line = String::new();
        let bytes = stdin
            .read_line(&mut line)
            .expect("stdin should read a line");

        if bytes == 0 {
            if !buffer.trim().is_empty() {
                eval_and_print(&mut session, &buffer);
            }
            println!();
            break;
        }

        let trimmed = line.trim().to_owned();
        if buffer.is_empty() && trimmed.is_empty() {
            continue;
        }

        if buffer.is_empty()
            && let Some(action) = session.handle_command(&trimmed)
        {
            match action {
                Ok(ReplAction::Continue(lines)) => print_lines(lines),
                Ok(ReplAction::Exit) => break,
                Err(message) => eprintln!("{message}"),
            }
            continue;
        }

        let line = if line.ends_with('\n') {
            line
        } else {
            format!("{line}\n")
        };

        if in_multiline && trimmed.is_empty() {
            eval_and_print(&mut session, &buffer);
            buffer.clear();
            in_multiline = false;
            saw_indented_line = false;
            continue;
        }

        let indent = leading_indent(&line);
        if indent > 0 {
            saw_indented_line = true;
        }

        buffer.push_str(&line);

        if !in_multiline && trimmed.ends_with(':') {
            in_multiline = true;
            continue;
        }

        if in_multiline {
            if saw_indented_line && indent == 0 && !trimmed.ends_with(':') {
                eval_and_print(&mut session, &buffer);
                buffer.clear();
                in_multiline = false;
                saw_indented_line = false;
            }
            continue;
        }

        eval_and_print(&mut session, &buffer);
        buffer.clear();
    }

    Ok(())
}

impl ReplSession {
    pub fn new() -> Self {
        Self {
            evaluator: Evaluator::new(),
            typecheck_history: String::new(),
        }
    }

    pub fn reset(&mut self) {
        self.evaluator = Evaluator::new();
        self.typecheck_history.clear();
    }

    pub fn history(&self) -> &str {
        &self.typecheck_history
    }

    pub fn eval_source(&mut self, source: &str, echo_expressions: bool) -> FyrResult<Vec<String>> {
        let source = ensure_trailing_newline(source);
        let typecheck_source = format!("{}{source}", self.typecheck_history);
        let typecheck_program = parse_source(&typecheck_source)?;
        typecheck::check(&typecheck_program)?;

        let program = parse_source(&source)?;
        let mut lines = Vec::new();

        self.evaluator.predefine_declarations(&program.statements)?;

        for statement in program.statements {
            if is_declaration(&statement) {
                continue;
            }

            let should_echo = echo_expressions && matches!(statement, Statement::Expr { .. });
            let value = self.evaluator.eval_statement(&statement)?;
            lines.extend(self.evaluator.take_outputs());

            if should_echo && value != Value::Unit {
                lines.push(value.to_string());
            }
        }

        self.typecheck_history.push_str(&source);
        Ok(lines)
    }

    fn handle_command(&mut self, trimmed: &str) -> Option<Result<ReplAction, String>> {
        match trimmed {
            ":help" => Some(Ok(ReplAction::Continue(help_lines()))),
            ":quit" | ":q" | "exit" => Some(Ok(ReplAction::Exit)),
            ":reset" => {
                self.reset();
                Some(Ok(ReplAction::Continue(vec!["session reset".to_owned()])))
            }
            ":history" => Some(Ok(ReplAction::Continue(self.history_lines()))),
            ":load" => Some(Err(":load expects a Rainbow source path".to_owned())),
            command if command.starts_with(":load ") => Some(self.load_command(command)),
            command if command.starts_with(':') => Some(Err(format!(
                "unknown REPL command '{command}'. Type :help for commands."
            ))),
            _ => None,
        }
    }

    fn load_command(&mut self, command: &str) -> Result<ReplAction, String> {
        let path = command.trim_start_matches(":load").trim();
        if path.is_empty() {
            return Err(":load expects a Rainbow source path".to_owned());
        }

        let source =
            fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))?;
        let mut lines = self
            .eval_source(&source, false)
            .map_err(|error| error.to_string())?;
        lines.push(format!("loaded {path}"));
        Ok(ReplAction::Continue(lines))
    }

    fn history_lines(&self) -> Vec<String> {
        let history = self.history().trim_end();
        if history.is_empty() {
            return vec!["(empty)".to_owned()];
        }

        history.lines().map(ToOwned::to_owned).collect()
    }
}

impl Default for ReplSession {
    fn default() -> Self {
        Self::new()
    }
}

fn eval_and_print(session: &mut ReplSession, source: &str) {
    match session.eval_source(source, true) {
        Ok(lines) => print_lines(lines),
        Err(error) => eprintln!("{error}"),
    }
}

fn print_lines(lines: Vec<String>) {
    for line in lines {
        println!("{line}");
    }
}

fn parse_source(source: &str) -> FyrResult<Program> {
    let tokens = lexer::lex(source)?;
    parser::parse(&tokens)
}

fn is_declaration(statement: &Statement) -> bool {
    matches!(
        statement,
        Statement::Struct { .. } | Statement::Fn { .. } | Statement::Import { .. }
    )
}

fn ensure_trailing_newline(source: &str) -> String {
    if source.ends_with('\n') {
        source.to_owned()
    } else {
        format!("{source}\n")
    }
}

fn help_lines() -> Vec<String> {
    vec![
        "Commands:".to_owned(),
        "  :help             show this help".to_owned(),
        "  :quit, :q, exit   exit the REPL".to_owned(),
        "  :reset            clear bindings and history".to_owned(),
        "  :history          show accepted source".to_owned(),
        "  :load <file>      run a Rainbow file in this session".to_owned(),
        "  let x = 42        bind a value".to_owned(),
        "  print(x)          print a value".to_owned(),
        "  blank line        submit a multi-line block".to_owned(),
    ]
}

fn leading_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

#[cfg(test)]
mod tests {
    use std::process;

    use super::*;

    #[test]
    fn echoes_expression_values_and_records_history() {
        let mut session = ReplSession::new();

        let bind_output = session
            .eval_source("let answer = 40 + 2\n", true)
            .expect("binding should evaluate");
        assert_eq!(bind_output, Vec::<String>::new());

        let echo_output = session
            .eval_source("answer\n", true)
            .expect("expression should evaluate");
        assert_eq!(echo_output, vec!["42"]);

        assert_eq!(session.history(), "let answer = 40 + 2\nanswer\n");
    }

    #[test]
    fn predefines_chunk_declarations_before_evaluating_repl_source() {
        let mut session = ReplSession::new();
        let source = r#"
print(double(21))
fn double(value: i64) -> i64:
    return value * 2
"#;

        let output = session
            .eval_source(source, false)
            .expect("chunk should use forward function declaration");

        assert_eq!(output, vec!["42"]);
    }

    #[test]
    fn reset_command_clears_values_and_history() {
        let mut session = ReplSession::new();
        session
            .eval_source("let answer = 42\n", true)
            .expect("binding should evaluate");

        let reset = session
            .handle_command(":reset")
            .expect("reset should be a command")
            .expect("reset should pass");
        assert_eq!(
            reset,
            ReplAction::Continue(vec!["session reset".to_owned()])
        );
        assert_eq!(session.history(), "");

        let error = session
            .eval_source("answer\n", true)
            .expect_err("reset should clear bindings");
        assert!(error.message.contains("unknown binding 'answer'"));
    }

    #[test]
    fn history_command_shows_accepted_source() {
        let mut session = ReplSession::new();
        session
            .eval_source("let answer = 42\nanswer\n", true)
            .expect("source should evaluate");

        let history = session
            .handle_command(":history")
            .expect("history should be a command")
            .expect("history should pass");

        assert_eq!(
            history,
            ReplAction::Continue(vec!["let answer = 42".to_owned(), "answer".to_owned()])
        );
    }

    #[test]
    fn load_command_runs_file_without_expression_echo() {
        let mut session = ReplSession::new();
        let path =
            std::env::temp_dir().join(format!("fyr_repl_load_{}_{}.fyr", process::id(), "smoke"));
        fs::write(&path, "let answer = 40 + 2\nanswer\nprint(answer)\n")
            .expect("test source should write");

        let load = session
            .handle_command(&format!(":load {}", path.display()))
            .expect("load should be a command")
            .expect("load should pass");

        assert_eq!(
            load,
            ReplAction::Continue(vec!["42".to_owned(), format!("loaded {}", path.display())])
        );
        assert_eq!(
            session.history(),
            "let answer = 40 + 2\nanswer\nprint(answer)\n"
        );

        fs::remove_file(path).expect("test source should clean up");
    }
}

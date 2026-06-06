use std::io::{self, Write};

use crate::ast::Statement;
use crate::diagnostic::FyrResult;
use crate::eval::{Evaluator, Value};
use crate::lexer;
use crate::parser;
use crate::typecheck;

pub fn start() -> FyrResult<()> {
    let mut evaluator = Evaluator::new();
    let mut typecheck_history = String::new();
    let stdin = io::stdin();
    let mut buffer = String::new();
    let mut in_multiline = false;
    let mut saw_indented_line = false;

    println!("Fyr 0.1.0 bootstrap REPL");
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
                eval_repl_source(&mut evaluator, &mut typecheck_history, &buffer);
            }
            println!();
            break;
        }

        let trimmed = line.trim().to_owned();
        if buffer.is_empty() && trimmed.is_empty() {
            continue;
        }

        if buffer.is_empty() {
            match trimmed.as_str() {
                ":help" => {
                    println!("Commands:");
                    println!("  :help        show this help");
                    println!("  :quit        exit the REPL");
                    println!("  let x = 42   bind a value");
                    println!("  print(x)     print a value");
                    println!("  blank line   submit a multi-line block");
                    continue;
                }
                ":quit" | ":q" | "exit" => break,
                _ => {}
            }
        }

        let line = if line.ends_with('\n') {
            line
        } else {
            format!("{line}\n")
        };

        if in_multiline && trimmed.is_empty() {
            eval_repl_source(&mut evaluator, &mut typecheck_history, &buffer);
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
                eval_repl_source(&mut evaluator, &mut typecheck_history, &buffer);
                buffer.clear();
                in_multiline = false;
                saw_indented_line = false;
            }
            continue;
        }

        eval_repl_source(&mut evaluator, &mut typecheck_history, &buffer);
        buffer.clear();
    }

    Ok(())
}

fn eval_repl_source(evaluator: &mut Evaluator, typecheck_history: &mut String, source: &str) {
    let typecheck_source = format!("{typecheck_history}{source}");
    let tokens = match lexer::lex(&typecheck_source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    let program = match parser::parse(&tokens) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    if let Err(error) = typecheck::check(&program) {
        eprintln!("{error}");
        return;
    }

    let tokens = match lexer::lex(source) {
        Ok(tokens) => tokens,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    let program = match parser::parse(&tokens) {
        Ok(program) => program,
        Err(error) => {
            eprintln!("{error}");
            return;
        }
    };

    for statement in program.statements {
        let should_echo = matches!(statement, Statement::Expr(_));

        match evaluator.eval_statement(&statement) {
            Ok(value) => {
                for line in evaluator.take_outputs() {
                    println!("{line}");
                }

                if should_echo && value != Value::Unit {
                    println!("{value}");
                }
            }
            Err(error) => {
                eprintln!("{error}");
                return;
            }
        }
    }

    typecheck_history.push_str(source);
}

fn leading_indent(line: &str) -> usize {
    line.chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

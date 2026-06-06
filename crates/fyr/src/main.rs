use std::env;
use std::fs;
use std::path::Path;
use std::process::ExitCode;

use fyr::{check_source, repl, run_source};

fn main() -> ExitCode {
    match run_cli() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_cli() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    match args.as_slice() {
        [] => repl::start().map_err(|error| error.to_string()),
        [_] => repl::start().map_err(|error| error.to_string()),
        [_, command] if command == "repl" => repl::start().map_err(|error| error.to_string()),
        [_, command] if command == "help" || command == "--help" || command == "-h" => {
            print_help();
            Ok(())
        }
        [_, command] if command == "version" || command == "--version" || command == "-V" => {
            println!("fyr 0.1.0");
            Ok(())
        }
        [_, command] if command == "doctor" => {
            print_doctor()?;
            Ok(())
        }
        [_, command, path] if command == "check" => {
            let source = read_source(path)?;
            check_source(&source).map_err(|error| error.to_string())?;
            println!("{path}: ok");
            Ok(())
        }
        [_, command, path] if command == "run" => {
            let source = read_source(path)?;
            let result = run_source(&source).map_err(|error| error.to_string())?;
            for line in result.outputs {
                println!("{line}");
            }
            Ok(())
        }
        [_, command, path] if command == "test" => {
            let source = read_source(path)?;
            let result = run_source(&source).map_err(|error| error.to_string())?;
            for line in result.outputs {
                println!("{line}");
            }
            println!("{path}: pass");
            Ok(())
        }
        [_, unknown, ..] => Err(format!(
            "unknown command '{unknown}'. Run `fyr help` for usage."
        )),
    }
}

fn read_source(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("failed to read {path}: {error}"))
}

fn print_help() {
    println!("Fyr programming language bootstrap");
    println!();
    println!("Usage:");
    println!("  fyr              Start the REPL");
    println!("  fyr repl         Start the REPL");
    println!("  fyr run <file>   Run a Fyr source file");
    println!("  fyr check <file> Parse-check a Fyr source file");
    println!("  fyr test <file>  Run a Fyr assertion test file");
    println!("  fyr doctor       Show command/install diagnostics");
    println!("  fyr version      Print the Fyr version");
}

fn print_doctor() -> Result<(), String> {
    let exe = env::current_exe().map_err(|error| format!("failed to locate fyr: {error}"))?;
    let cwd = env::current_dir().map_err(|error| format!("failed to read cwd: {error}"))?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| "failed to locate fyr binary directory".to_owned())?;
    let path = env::var_os("PATH").unwrap_or_default();
    let on_path = env::split_paths(&path).any(|entry| same_path(&entry, exe_dir));

    println!("fyr doctor");
    println!("  version: 0.1.0");
    println!("  executable: {}", exe.display());
    println!("  cwd: {}", cwd.display());
    println!("  binary directory on PATH: {on_path}");

    if !on_path {
        println!("  hint: add {} to PATH", exe_dir.display());
    }

    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

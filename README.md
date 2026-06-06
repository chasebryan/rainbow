# Fyr

Fyr is a new systems programming language aiming for native performance, strong safety, and a small, readable surface.

The long-term goal is direct:

- fast like C
- secure like Rust
- simple like Python

This repository begins with a working bootstrap: a Rust implementation of the `fyr` command, a tiny parser/evaluator, `fyr run`, `fyr check`, and a terminal REPL.

## Try It

```sh
cargo run -p fyr -- run examples/hello.fyr
cargo run -p fyr -- run examples/fib.fyr
cargo run -p fyr -- run examples/sum.fyr
cargo run -p fyr -- check examples/hello.fyr
cargo run -p fyr
```

Inside the REPL:

```fyr
let answer = 40 + 2
answer
print("Fyr is alive")
```

Functions use typed signatures and Python-style indented bodies:

```fyr
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)

print(fib(10))
```

Loops use explicit mutable bindings:

```fyr
var total = 0
var i = 1

while i <= 100:
    total = total + i
    i = i + 1

print(total)
```

## Current Language Slice

The bootstrap supports:

- integer, boolean, and string literals
- `let` bindings
- explicit mutable `var` bindings and assignment
- arithmetic and comparison operators
- boolean `&&`, `||`, and `!`
- string concatenation with `+`
- typed function signatures with Python-style indented bodies
- recursive function calls
- checked function calls and return types
- `if` / `else` expressions
- `while` loops
- built-in `print(value)` and `type(value)`
- one-statement-per-line scripts

The bootstrap typechecker enforces `i64`, `bool`, `str`, and `unit` annotations across function calls, return values, branch expressions, assignments, and supported operators.

## Direction

Fyr will grow in stages:

1. bootstrap interpreter and REPL
2. expanded static type checker and inference
3. ownership and safety checker
4. native backend
5. standard library
6. package manager and build system
7. the Fyr book

The repo should always keep a runnable language at the center. Design documents and book chapters should describe behavior that either exists or is actively being implemented.

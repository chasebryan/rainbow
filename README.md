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
cargo run -p fyr -- run examples/control.fyr
cargo run -p fyr -- run examples/point.fyr
cargo run -p fyr -- run examples/arrays.fyr
cargo run -p fyr -- run examples/range.fyr
cargo run -p fyr -- check examples/hello.fyr
cargo run -p fyr -- test examples/assertions.fyr
cargo run -p fyr
```

## Install Locally

Install the current checkout as a `fyr` command:

```sh
cargo install --path crates/fyr --force
```

Then it can run from any path:

```sh
fyr doctor
fyr run /absolute/path/to/file.fyr
fyr test /absolute/path/to/test.fyr
fyr
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

Functions can return early from loops:

```fyr
fn first_multiple_of_seven(limit: i64) -> i64:
    var i = 1
    while i <= limit:
        if i % 7 == 0:
            return i
        i = i + 1
    return -1
```

Structs define nominal data:

```fyr
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
print(p.x + p.y)
```

Arrays are homogeneous and bounds-checked:

```fyr
fn sum(values: [i64]) -> i64:
    var total = 0
    for value in values:
        total = total + value
    return total

let values = [3, 5, 8, 13]
let more_values = values + [21]
let empty: [i64] = []
print(sum(more_values))
print(len(empty))
```

Use `range` for counted loops:

```fyr
var total = 0
for value in range(1, 11):
    total = total + value

print(total)
```

Bindings may include explicit annotations when clarity or an empty literal needs them:

```fyr
let name: str = "Fyr"
var scores: [i64] = []
```

Assertions make Fyr files testable:

```fyr
assert(sum([3, 5, 8, 13]) == 29, "sum should add every element")
assert([1, 2, 3] == [1, 2, 3])
assert(range(5)[4] == 4)
```

Run assertion files with:

```sh
fyr test examples/assertions.fyr
```

## Current Language Slice

The bootstrap supports:

- integer, boolean, and string literals
- inferred and explicitly annotated `let` bindings
- inferred and explicitly annotated mutable `var` bindings and assignment
- arithmetic and comparison operators
- value equality for primitives, arrays, structs, and `unit`
- boolean `&&`, `||`, and `!`
- string concatenation with `+`
- typed function signatures with Python-style indented bodies
- recursive function calls
- checked function calls and return types
- statement-style `if` blocks and value-producing `if` / `else` branches
- `while` loops and array `for value in values` loops
- `return`, `break`, and `continue`
- `struct` declarations, struct literals, and field access
- homogeneous array literals, `[T]` annotations, typed empty arrays, concatenation with `+`, checked indexing, and `len(array)`
- built-in `print(value)`, `type(value)`, `len(value)`, end-exclusive `range(...)`, and `assert(...)`
- `fyr test <file>` assertion-file execution
- one-statement-per-line scripts

The bootstrap typechecker enforces `i64`, `bool`, `str`, `unit`, struct, and array types across function calls, return values, branch expressions, assignments, equality, indexing, and supported operators.

Bootstrap `range` materializes an array and currently caps each range at 1,000,000 elements. Later iterator work should make counted loops lazy.

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

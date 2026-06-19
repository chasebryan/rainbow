# Rainbow

Rainbow is a new systems programming language aiming for native performance, strong safety, and a beautiful, readable surface.

The language goal is direct:

- fast like C
- secure like Rust
- simple like Python
- readable like Ruby
- expressive like the best functional languages
- friendly like the best interactive tools

Rainbow is not trying to crown itself with a grand title. It is a coherent language effort that studies the best proven ideas from systems, functional, dynamic, data, and interactive programming while keeping programs visually calm. The concrete ambition is tracked in [docs/RAINBOW_NORTH_STAR.md](docs/RAINBOW_NORTH_STAR.md).

This repository begins with a working bootstrap: a Rust implementation of the `fyr` command, a tiny parser/evaluator, `fyr run`, `fyr check`, `fyr fmt`, and a terminal REPL. The current command, manifest, and file extension are still named `fyr` until the toolchain rename is completed deliberately.

Rainbow is made under the `chasebryan` name. The target repository is `https://github.com/chasebryan/rainbow`.

## Try It

```sh
cargo run -p fyr -- run examples/hello.fyr
cargo run -p fyr -- run examples/fib.fyr
cargo run -p fyr -- run examples/sum.fyr
cargo run -p fyr -- run examples/control.fyr
cargo run -p fyr -- run examples/point.fyr
cargo run -p fyr -- run examples/arrays.fyr
cargo run -p fyr -- run examples/range.fyr
cargo run -p fyr -- run examples/strings.fyr
cargo run -p fyr -- run examples/pipeline.fyr
cargo run -p fyr -- run examples/floats.fyr
cargo run -p fyr -- run examples/nil.fyr
cargo run -p fyr -- run examples/enums.fyr
cargo run -p fyr -- run examples/project/src/main.fyr
cargo run -p fyr -- check examples
cargo run -p fyr -- fmt --check examples
cargo run -p fyr -- test examples
cargo run -p fyr
```

## Rainbow Charter

Rainbow should be beautiful in the way a good proof, a good circuit, or a good sentence is beautiful: clear structure, low noise, precise meaning, and no hidden traps.

The design test for every feature is whether it earns a color in the spectrum:

- C and Zig: predictable native performance, explicit layout, and direct systems reach
- Rust: memory safety, data-race prevention, algebraic data, and fearless refactoring
- Python and Ruby: readable everyday code, fast feedback, and humane APIs
- ML, Haskell, and F#: expressions, pattern matching, inference, and types that model truth
- Lisp and Smalltalk: interactive development, language extension, and live exploration
- Erlang, Elixir, Go, and Pony: practical concurrency, supervision, and isolated work
- Swift, Kotlin, and TypeScript: modern tooling, helpful diagnostics, and productive application code
- SQL, R, and Julia: first-class data work, numerics, and interactive analysis
- Shell and Make: composable tools and honest build automation

The hard performance/safety/readability bar lives in [docs/RAINBOW_NORTH_STAR.md](docs/RAINBOW_NORTH_STAR.md). The language synthesis map lives in [docs/LANGUAGE_SYNTHESIS.md](docs/LANGUAGE_SYNTHESIS.md).

## Install Locally

Install the current checkout as a `fyr` command:

```sh
cargo install --path crates/fyr --force
```

Then it can run from any path:

```sh
fyr doctor
fyr new hello-rainbow
cd hello-rainbow
fyr run
fyr check
fyr fmt --check
fyr test
fyr build
fyr /absolute/path/to/file.fyr
fyr run /absolute/path/to/file.fyr
fyr check /absolute/path/to/file-or-dir
fyr fmt --check /absolute/path/to/file-or-dir
fyr fmt /absolute/path/to/file-or-dir
fyr test /absolute/path/to/test-file-or-dir
fyr
```

Directory inputs are searched recursively for `.fyr` files.
The bootstrap formatter writes files in place by default and checks without writing when passed `--check`; it preserves line comments while canonicalizing spacing, indentation, and expression layout.

## Start a Project

Create a project in a new directory:

```sh
fyr new hello-rainbow
cd hello-rainbow
fyr run
fyr check
fyr test
fyr fmt --check
fyr build
```

`fyr init` writes the same files into the current directory or a directory you pass:

```sh
fyr init
fyr init tools/demo
```

A Rainbow project currently has a small `fyr.toml` manifest:

```toml
name = "hello-rainbow"
main = "src/main.fyr"
```

When run inside a project, `fyr run` uses the manifest `main` file, `fyr check` and `fyr fmt` default to project sources plus tests, and `fyr test` defaults to the project `tests` directory.

Split project code across files with relative imports:

```fyr
import "lib.fyr"
print(greeting("Rainbow"))
```

Import paths are string literals that must be relative `.fyr` files. The CLI resolves imports before typechecking or running, detects cycles, and includes repeated imports once per root file. Missing or invalid import paths report the import statement that failed. Syntax and formatting errors include the file path that failed, including imported files; type and runtime errors fall back to the original source statement location, including imported statements. File-backed diagnostics include nearby source lines with a caret underline when the source is available. Inside a project, imports are confined to the nearest `fyr.toml` project root.

Build a project into a checked, import-flattened Rainbow bundle:

```sh
fyr build
fyr build --out dist/app.fyr
fyr run build/main.fyr
```

For a project, the default output is `build/main.fyr`. For a loose input file, `fyr build src/main.fyr` writes `src/main.bundle.fyr` unless `--out` is passed. This bootstrap build artifact is still Rainbow source; native code generation remains a later compiler layer.

Inside the REPL:

```fyr
let answer = 40 + 2
answer
print("Rainbow is alive")
```

The REPL keeps accepted bindings and declarations alive between submissions. It also has terminal commands for exploration:

```text
:help
:load /absolute/path/to/file.fyr
:history
:reset
:quit
```

`:load` runs a Rainbow source file inside the current session, which makes it useful for loading helpers and then experimenting with them interactively. Submitted chunks and loaded files predeclare their top-level functions before evaluating other statements, matching normal source-file behavior.

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

Use `elif` for readable multi-way branching:

```fyr
fn size_label(value: i64) -> str:
    if value < 0:
        return "negative"
    elif value == 0:
        return "zero"
    elif value == 1:
        return "one"
    else:
        return "many"
```

Structs define nominal data:

```fyr
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
print(p.x + p.y)
```

Enums define closed state sets. Variants can be unit values or carry one typed payload, and exhaustive `match` expressions plus enum-pattern `if let` branches can bind payloads safely:

```fyr
enum Status:
    Pending
    Ready
    Failed(str)

fn label(status: Status) -> str:
    return match status:
        Status.Pending:
            "pending"
        Status.Ready:
            "ready"
        Status.Failed(message):
            message

print(label(Status.Ready))
print(label(Status.Failed("blocked")))

if let Status.Failed(message) = Status.Failed("blocked"):
    print(message)
```

Arrays are homogeneous and bounds-checked:

```fyr
fn sum(values: [i64]) -> i64:
    var total = 0
    for value in values:
        total = total + value
    return total

let values = [3, 5, 8, 13]
let more_values = append(values, 21)
let middle_values = slice(more_values, 1, 4)
let safe_missing = get(more_values, 99, -1)
let found_index = find(more_values, 13)
let value_count = count(more_values, 13)
let reversed_values = reverse(more_values)
let first_value = first(more_values, -1)
let last_value = last(more_values, -1)
let empty: [i64] = []
print(sum(more_values))
print(middle_values)
print(safe_missing)
print(found_index)
print(value_count)
print(reversed_values)
print(first_value)
print(last_value)
print(len(empty))
print(is_empty(empty))
```

Strings are indexed and iterable by character:

```fyr
fn rebuild(text: str) -> str:
    var rebuilt = ""
    for ch in text:
        rebuilt = rebuilt + ch
    return rebuilt

let name = "Rainbow"
let phrase = "  Fast Secure Simple  "
let cleaned = trim(phrase)
let words = split(lower(cleaned), " ")
print(name[0])
print(name[1])
print(name[2])
print(rebuild(name))
print(cleaned)
print(join(words, "-"))
print(upper(name))
print(starts_with(cleaned, "Fast"))
print(ends_with(cleaned, "Simple"))
print(replace(cleaned, "Simple", "Readable"))
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
let name: str = "Rainbow"
var scores: [i64] = []
```

Assertions make Rainbow files testable:

```fyr
assert(sum([3, 5, 8, 13]) == 29, "sum should add every element")
assert(is_empty([]))
assert(append([3, 5, 8], 13) == [3, 5, 8, 13])
assert(slice([3, 5, 8, 13], 1, 3) == [5, 8])
assert(get([3, 5, 8], 99, -1) == -1)
assert(reverse([3, 5, 8]) == [8, 5, 3])
assert(first([3, 5, 8], -1) == 3)
assert(last([3, 5, 8], -1) == 8)
assert(find([3, 5, 8], 8) == 2)
assert(count([3, 5, 3, 8, 3], 3) == 3)
assert(contains([3, 5, 8, 13], 8))
assert(not contains([3, 5, 8, 13], 21) and contains([3, 5, 8, 13], 8))
assert(contains("beautiful Rainbow", "Rainbow"))
assert("Rainbow"[0] == "R")
assert("Rainbow"[1] == "a")
assert(trim("  Rainbow  ") == "Rainbow")
assert(lower("RAINBOW") == "rainbow")
assert(upper("rainbow") == "RAINBOW")
assert(starts_with("Rainbow", "R"))
assert(ends_with("Rainbow", "w"))
assert(replace("Fast C", "C", "Rainbow") == "Fast Rainbow")
assert(split("fast secure simple", " ") == ["fast", "secure", "simple"])
assert(join(["fast", "secure", "simple"], "-") == "fast-secure-simple")
assert(slice("beautiful Rainbow", 0, 9) == "beautiful")
assert(get("Rainbow", 1, "?") == "a")
assert(reverse("Rainbow") == "wobniaR")
assert(first("Rainbow", "?") == "R")
assert(last("Rainbow", "?") == "w")
assert(find("beautiful Rainbow", "Rainbow") == 10)
assert(count("beautiful Rainbow beautiful", "beautiful") == 2)
assert(is_empty(""))
assert([1, 2, 3] == [1, 2, 3])
assert(range(5)[4] == 4)
```

Run assertion files with:

```sh
fyr test examples
```

## Current Language Slice

The bootstrap supports:

- integer, floating-point, boolean, and string literals
- inferred and explicitly annotated `let` bindings
- inferred and explicitly annotated mutable `var` bindings and assignment
- checked integer arithmetic plus finite `f64` arithmetic and comparison operators
- explicit checked numeric conversions with `i64(value)` and `f64(value)`
- value equality for primitives, arrays, structs, enums, and `unit`
- `nil` values with explicit nullable `T?` annotations, safe `value ?? fallback` coalescing, and scoped `if let value = maybe:` unwrapping
- boolean `and`, `or`, and `not`, with `&&`, `||`, and `!` aliases
- string concatenation with `+`
- pipeline calls with `value |> function` and `value |> function(extra, args)` for readable left-to-right transformations
- typed function signatures with Python-style indented bodies
- recursive function calls and local function declarations after the declaration point
- checked function calls and return types
- statement-style `if` / `elif` / `else` blocks, scoped `if let` / `elif let` nullable unwrapping and enum-pattern branching, and value-producing `if` / `elif` / `else` branches
- `while` loops plus array and string `for value in values` loops
- `return`, `break`, and `continue`
- `struct` declarations, struct literals, and field access
- `enum` declarations with nominal unit and payload variants such as `Status.Ready` and `Status.Failed("blocked")`
- exhaustive `match` expressions and enum-pattern `if let` branches for enum variants, with payload bindings and `else` fallback arms when desired
- homogeneous array literals, `[T]` and `[T?]` annotations, typed empty arrays, append, reverse, first/last reads, concatenation with `+`, checked indexing, fallback reads, checked slicing, search/count helpers, emptiness checks, and `len(array)`
- checked string indexing, character iteration, concatenation, containment, slicing, fallback reads, search/count helpers, split/join helpers, trim/case helpers, prefix/suffix checks, replacement, reverse, first/last reads, emptiness checks, and `len(str)`
- relative file imports with `import "path/to/file.fyr"` for multi-file programs and projects
- built-in `print(value)`, `type(value)`, `len(value)`, `is_empty(value)`, `get(value, index, default)`, `first(value, default)`, `last(value, default)`, `reverse(value)`, `find(value, item)`, `count(value, item)`, `append(array, value)`, `contains(value, item)`, `slice(value, start, end)`, `split(text, separator)`, `join(parts, separator)`, `trim(text)`, `lower(text)`, `upper(text)`, `starts_with(text, prefix)`, `ends_with(text, suffix)`, `replace(text, old, new)`, end-exclusive `range(...)`, and `assert(...)`
- a persistent terminal REPL with `:help`, `:load <file>`, `:history`, `:reset`, and `:quit`
- `fyr init [dir]` and `fyr new <dir>` project scaffolding with `fyr.toml`, `src/main.fyr`, and `tests/main.fyr`
- project-aware `fyr run`, `fyr check`, `fyr fmt`, and `fyr test` defaults when run below a `fyr.toml`
- `fyr build [file]` checked, import-flattened Rainbow bundle generation, with `--out <file>` for custom output
- `fyr fmt <path...>` in-place formatting and `fyr fmt --check <path...>` formatting checks
- `fyr test <path...>` assertion-file execution
- one-statement-per-line scripts

The bootstrap typechecker enforces `i64`, `f64`, `bool`, `str`, `unit`, struct, enum, array, and nullable `T?` types across function calls, return values, branch expressions, `match` arms, enum payload constructors, assignments, equality, indexing, and supported operators. `i64` and `f64` do not implicitly mix yet; write `f64(count)` or `i64(score)` when a conversion is intentional. `i64(f64_value)` only accepts whole finite values in the exact integer range, and `f64(i64_value)` rejects precision-losing integers. `nil` requires an explicit nullable destination unless another branch or expected type supplies one. Empty array branch results can use a sibling array branch as their type, while a bare `let values = []` still needs an annotation or another type hint. Use `maybe ?? fallback` to recover a concrete value with a fallback, `if let value = maybe:` to bind the inner nullable value only inside the present branch, or `if let Result.Ok(value) = result:` to bind an enum payload only for the matching variant. Runtime integer arithmetic fails on overflow, division by zero, and remainder by zero instead of wrapping; `f64` arithmetic rejects divide/remainder by zero and non-finite results.

The checker also rejects ambiguous declaration shapes such as duplicate bindings in the same scope, duplicate function parameters, duplicate struct fields, duplicate enum variants, duplicate or missing enum match arms, and value/function names that reuse a nominal type name.

Bootstrap `range` materializes an array and currently caps each range at 1,000,000 elements. Later iterator work should make counted loops lazy.

## Direction

Rainbow will grow in stages:

1. bootstrap interpreter and REPL
2. expanded static type checker and inference
3. ownership and safety checker
4. native backend
5. standard library
6. package manager and build system
7. the Rainbow book

The repo should always keep a runnable language at the center. Design documents and book chapters should describe behavior that either exists or is actively being implemented.

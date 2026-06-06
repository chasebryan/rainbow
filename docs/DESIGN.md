# Fyr Design Charter

Fyr is a native systems language with three product constraints:

- C-class runtime performance
- Rust-class memory and concurrency safety
- Python-class readability for common code

## Safety Model

Safe Fyr should reject:

- null dereferences
- use-after-free
- data races
- unchecked integer and memory edge cases where practical
- implicit lossy conversions
- undefined behavior

Unsafe Fyr will exist, but it must be explicit, narrow, and auditable.

## Syntax Direction

Fyr should favor readable, low-noise syntax:

```fyr
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)
```

The bootstrap implementation now supports typed function signatures, local function declarations after the declaration point, optional binding annotations, nominal structs with field access, value equality for data, homogeneous arrays with checked append, indexing, fallback reads, search/count helpers, slicing, and emptiness checks, checked integer arithmetic, concatenation, containment checks, and iteration, string containment, fallback reads, search/count helpers, slicing, and emptiness checks, readable boolean operators, end-exclusive `range` loops, explicit mutable `var` bindings, static checks for calls and primitive operations, declaration hygiene for same-scope bindings, function parameters, and struct fields, Python-style indented blocks, statement-style `if` / `elif` / `else` blocks, expression-style `if` / `elif` / `else` branches, `while` loops, and explicit `return` / `break` / `continue` exits. Fuller inference, ownership, and native code generation remain upcoming compiler layers.

## Toolchain Direction

The `fyr` command should become the single daily entrypoint:

```sh
fyr
fyr run app.fyr
fyr check src tests
fyr build
fyr test tests
fyr fmt
```

The first implementation uses an interpreter. The planned native path is:

```text
source -> tokens -> AST -> types -> safety IR -> optimization IR -> native backend
```

Cranelift is the preferred early native backend because it gives Fyr fast native execution without making LLVM the first milestone.

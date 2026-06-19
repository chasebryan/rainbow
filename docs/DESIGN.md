# Rainbow Design Charter

Rainbow is a native systems language with five product constraints:

- C-class runtime performance
- Rust-class memory and concurrency safety
- Python-class readability for common code
- ML-class modeling through types and patterns
- Smalltalk/Lisp-class interactive exploration without losing static rigor

Rainbow studies the best ideas from many traditions and keeps only the ones that make programs safer, faster, clearer, or more beautiful. The hard north-star bar is tracked in [RAINBOW_NORTH_STAR.md](RAINBOW_NORTH_STAR.md), and the detailed synthesis map is tracked in [LANGUAGE_SYNTHESIS.md](LANGUAGE_SYNTHESIS.md).

## Safety Model

Safe Rainbow should reject:

- null dereferences
- use-after-free
- data races
- unchecked integer and memory edge cases where practical
- implicit lossy conversions
- undefined behavior

Unsafe Rainbow will exist, but it must be explicit, narrow, and auditable.

## Syntax Direction

Rainbow should favor readable, low-noise syntax:

```fyr
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)
```

The bootstrap implementation now supports typed function signatures, local function declarations after the declaration point, optional binding annotations, nominal structs with field access, unit and payload enums for closed state/data sets, exhaustive enum `match` expressions with payload bindings, enum-pattern `if let` branches, value equality for data, explicit `nil` with nullable `T?` types, scoped `if let` nullable unwrapping, homogeneous arrays with checked append, reverse, first/last reads, indexing, fallback reads, search/count helpers, slicing, and emptiness checks, checked integer arithmetic, finite `f64` arithmetic, concatenation, containment checks, and iteration, string containment, checked indexing, character iteration, split/join, trim/case helpers, prefix/suffix checks, replacement, reverse, first/last reads, fallback reads, search/count helpers, slicing, and emptiness checks, readable boolean operators, relative file imports, end-exclusive `range` loops, explicit mutable `var` bindings, static checks for calls and primitive operations, declaration hygiene for same-scope bindings, function parameters, struct fields, and enum variants, Python-style indented blocks, statement-style `if` / `elif` / `else` blocks, expression-style `if` / `elif` / `else` / `match` branches, `while` loops, explicit `return` / `break` / `continue` exits, a persistent REPL with load/history/reset commands, project scaffolding with `fyr.toml`, checked import-flattened bootstrap build artifacts, and comment-preserving `fyr fmt` formatting checks/writes. Fuller inference, ownership, and native code generation remain upcoming compiler layers.

Pipeline calls provide the first small Ruby/Elixir/F# inspired readability feature:

```fyr
let label = "  Rainbow  " |> trim |> lower |> replace("rainbow", "Rainbow")
```

`value |> function` is checked and evaluated as `function(value)`. `value |> function(extra, args)` is checked and evaluated as `function(value, extra, args)`. It is call syntax, not a dynamic dispatch escape hatch.

Closed state and result-like data sets use unit and payload enums:

```fyr
enum Status:
    Pending
    Ready
    Failed(str)

let status: Status = Status.Ready

let label = match status:
    Status.Pending:
        "pending"
    Status.Ready:
        "ready"
    Status.Failed(message):
        message
```

Variants are nominal values and can be compared, stored in homogeneous arrays, and handled with exhaustive `match` expressions. Payload variants use `Enum.Variant(value)` constructors, carry one typed value, and can bind that value inside a matching arm.

Single-variant branches can use the same pattern shape:

```fyr
if let Status.Failed(message) = status:
    print(message)
```

Numeric types are explicit:

```fyr
let count: i64 = 4
let ratio: f64 = 3.14
let total: f64 = f64(count) + ratio
```

`i64` and `f64` do not implicitly mix in the bootstrap. Use `f64(value)` and `i64(value)` when a conversion is intentional. Integer arithmetic remains overflow checked, floating-point arithmetic rejects divide/remainder by zero plus non-finite results, and numeric conversions reject fractional or precision-losing values instead of silently narrowing.

Nullable values use postfix `?` syntax:

```fyr
let missing: i64? = nil
let values: [i64?] = [nil, 42]
let recovered: i64 = missing ?? 0

if let value = values[1]:
    print(value)
```

Plain `T` values can flow into `T?`, and `nil` can only flow into nullable destinations. Flowing a `T?` directly back into `T` remains rejected. Use the `??` coalescing operator to provide a fallback and recover the inner `T` safely; the fallback is only evaluated when the left side is `nil`. Use `if let name = maybe:` to bind the inner value in a branch-local immutable name when the nullable value is present.

## Toolchain Direction

The `fyr` command should become the single daily entrypoint:

```sh
fyr
fyr new app
cd app
fyr run
fyr check
fyr fmt --check
fyr test
fyr build
fyr run app.fyr
fyr check src tests
fyr fmt --check src tests
fyr fmt src tests
fyr build
fyr test tests
```

The first import form is intentionally direct:

```fyr
import "lib.fyr"
```

Imports are relative `.fyr` files resolved before checking and execution. The bootstrap command detects import cycles, deduplicates repeated imports for each root file, reports missing or invalid import paths at the import statement, reports syntax failures with the source file path, preserves statement source paths for typechecker and runtime fallback diagnostics after import flattening, and prints nearby source-line caret snippets when the source file is available. Project imports are confined to the nearest `fyr.toml` root. `fyr build` currently emits a checked, formatted Rainbow source bundle with imports flattened; native object/executable artifacts remain the later backend milestone.

The first implementation uses an interpreter. The planned native path is:

```text
source -> tokens -> AST -> types -> safety IR -> optimization IR -> native backend
```

Cranelift is the preferred early native backend because it gives Rainbow fast native execution without making LLVM the first milestone.

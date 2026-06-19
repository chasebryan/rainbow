# Welcome to Rainbow

Rainbow is a programming language for people who want native speed, strong safety, and code that stays readable under pressure.

The current bootstrap command is `rainbow`, and source files use the `.rain` extension.

The first thing to learn is the terminal command:

```sh
rainbow
```

With no arguments, `rainbow` starts the REPL. That REPL is the front door to the language.

```rainbow
let answer = 40 + 2
answer
```

Accepted bindings stay available until the session is reset or closed. The REPL also has commands:

```text
:help
:load examples/hello.rain
:history
:reset
:quit
```

`:load` runs a Rainbow file in the current session, so you can load helpers and then keep experimenting. Top-level functions are predeclared inside each submitted chunk, matching normal source-file behavior.

When you want a project instead of a loose file, create one:

```sh
rainbow new hello-rainbow
cd hello-rainbow
rainbow run
rainbow check
rainbow test
rainbow fmt --check
rainbow build
```

The Rainbow project manifest is `rainbow.toml`:

```toml
name = "hello-rainbow"
main = "src/main.rain"
```

Inside a project, `rainbow run` uses the manifest `main` file. `rainbow check` and `rainbow fmt` default to project sources plus tests, and `rainbow test` defaults to the project `tests` directory.

Use imports when a project grows beyond one file:

```rainbow
import "lib.rain"
print(greeting("Rainbow"))
```

Imports use relative `.rain` paths. The command resolves imports before typechecking and running, catches import cycles, reports missing or invalid import paths at the import statement, reports syntax failures with the source file path, and keeps imported statement locations for type and runtime diagnostics. File-backed diagnostics also show nearby source lines with a caret underline. It only includes the same imported file once for each root file. Inside a project, imports stay inside the nearest `rainbow.toml` project root.

`rainbow build` writes a checked, import-flattened Rainbow source bundle:

```sh
rainbow build
rainbow run build/main.rain
```

The bootstrap build output is still Rainbow source. Later compiler stages will turn the same project shape toward native artifacts.

Rainbow functions use typed signatures and indented bodies:

```rainbow
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)

print(fib(10))
```

Functions can define local helpers after the values they need are in scope:

```rainbow
fn doubled(value: i64) -> i64:
    fn double(input: i64) -> i64:
        return input * 2

    return double(value)
```

Mutation is explicit:

```rainbow
var total = 0
var i = 1

while i <= 10:
    total = total + i
    i = i + 1

print(total)
```

Integer arithmetic is checked. Overflow, division by zero, and remainder by zero stop the program instead of wrapping. Decimal values use `f64`:

```rainbow
let radius: f64 = 2.5
let area = 3.14 * radius * radius
let samples: i64 = 3
let adjusted = f64(samples) + area
print(area)
```

Rainbow keeps `i64` and `f64` separate for now; write the type you want instead of relying on implicit numeric widening. Use `f64(count)` to convert an integer to a decimal value, and `i64(score)` to recover a whole decimal value. Those conversions are checked so fractional values and precision-losing integers fail instead of silently changing shape.

Use `nil` when a value can be absent, and mark that type with `?`:

```rainbow
fn score(ready: bool) -> i64?:
    if ready:
        return 42
    else:
        return nil

let missing: i64? = nil
let values: [i64?] = [missing, score(true)]
let safe_score: i64 = missing ?? 0

if let value = score(true):
    print(value)
else:
    print(0)
```

Plain `i64` values can flow into `i64?`, but `i64?` cannot flow directly back into `i64`. Use `value ?? fallback` to recover a concrete value safely; Rainbow only evaluates the fallback when the value is `nil`. Use `if let value = maybe:` when you want a scoped name for the present value; that name only exists inside the success branch.

Functions can return early:

```rainbow
fn first_multiple_of_seven(limit: i64) -> i64:
    var i = 1
    while i <= limit:
        if i % 7 == 0:
            return i
        i = i + 1
    return -1
```

Use `elif` when a branch has several named cases:

```rainbow
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

Structs name the shape of data:

```rainbow
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
print(p.x + p.y)
```

Enums name a closed set of states, and variants can carry one typed payload:

```rainbow
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

print(label)
```

Payload constructors use `Enum.Variant(value)`, and a matching arm can bind that payload:

```rainbow
enum Result:
    Ok(i64)
    Err(str)

let result = Result.Ok(42)
let value = match result:
    Result.Ok(number):
        number
    Result.Err(message):
        len(message)

print(value)

if let Result.Ok(number) = result:
    print(number)
```

Arrays collect values of one type:

```rainbow
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

```rainbow
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

Flow calls keep transformations readable from left to right. The value on the left becomes the first argument to the function on the right:

```rainbow
fn bracket(value: str, left: str, right: str) -> str:
    return left + value + right

let label = "  Rainbow  " then trim then lower then bracket("[", "]")
print(label)
```

For counted loops, use `range`. The end is not included:

```rainbow
var total = 0
for value in range(1, 11):
    total = total + value

print(total)
```

Most bindings can be inferred. Add an annotation when it makes the program clearer or when Rainbow cannot infer the type yet:

```rainbow
let name: str = "Rainbow"
var scores: [i64] = []
```

Assertions turn ordinary Rainbow files into tests:

```rainbow
assert(range(5)[4] == 4)
assert(contains([3, 5, 8, 13], 8))
assert(is_empty([]))
assert(append([3, 5, 8], 13) == [3, 5, 8, 13])
assert(slice([3, 5, 8, 13], 1, 3) == [5, 8])
assert(get([3, 5, 8], 99, -1) == -1)
assert(reverse([3, 5, 8]) == [8, 5, 3])
assert(first([3, 5, 8], -1) == 3)
assert(last([3, 5, 8], -1) == 8)
assert(find([3, 5, 8], 8) == 2)
assert(count([3, 5, 3, 8, 3], 3) == 3)
assert(not contains([3, 5, 8, 13], 21))
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
assert(total == 55, "total should match the counted loop")
```

Run them with:

```sh
rainbow test examples
```

Format Rainbow files with:

```sh
rainbow fmt --check examples
rainbow fmt examples
```

When `rainbow check`, `rainbow fmt`, or `rainbow test` receives a directory, it recursively finds `.rain` files. The bootstrap formatter preserves line comments while canonicalizing spacing, indentation, and expression layout.

The bootstrap version of Rainbow is intentionally small. Each chapter of this book should track real language behavior as the compiler grows.

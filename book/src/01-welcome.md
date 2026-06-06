# Welcome to Fyr

Fyr is a programming language for people who want native speed, strong safety, and code that stays readable under pressure.

The first thing to learn is the terminal command:

```sh
fyr
```

With no arguments, `fyr` starts the REPL. That REPL is the front door to the language.

```fyr
let answer = 40 + 2
answer
```

Fyr functions use typed signatures and indented bodies:

```fyr
fn fib(n: i64) -> i64:
    if n < 2:
        n
    else:
        fib(n - 1) + fib(n - 2)

print(fib(10))
```

Functions can define local helpers after the values they need are in scope:

```fyr
fn doubled(value: i64) -> i64:
    fn double(input: i64) -> i64:
        return input * 2

    return double(value)
```

Mutation is explicit:

```fyr
var total = 0
var i = 1

while i <= 10:
    total = total + i
    i = i + 1

print(total)
```

Integer arithmetic is checked. Overflow, division by zero, and remainder by zero stop the program instead of wrapping.

Functions can return early:

```fyr
fn first_multiple_of_seven(limit: i64) -> i64:
    var i = 1
    while i <= limit:
        if i % 7 == 0:
            return i
        i = i + 1
    return -1
```

Use `elif` when a branch has several named cases:

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

Structs name the shape of data:

```fyr
struct Point:
    x: i64
    y: i64

let p = Point { x: 3, y: 4 }
print(p.x + p.y)
```

Arrays collect values of one type:

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
let empty: [i64] = []
print(sum(more_values))
print(middle_values)
print(safe_missing)
print(found_index)
print(value_count)
print(len(empty))
print(is_empty(empty))
```

For counted loops, use `range`. The end is not included:

```fyr
var total = 0
for value in range(1, 11):
    total = total + value

print(total)
```

Most bindings can be inferred. Add an annotation when it makes the program clearer or when Fyr cannot infer the type yet:

```fyr
let name: str = "Fyr"
var scores: [i64] = []
```

Assertions turn ordinary Fyr files into tests:

```fyr
assert(range(5)[4] == 4)
assert(contains([3, 5, 8, 13], 8))
assert(is_empty([]))
assert(append([3, 5, 8], 13) == [3, 5, 8, 13])
assert(slice([3, 5, 8, 13], 1, 3) == [5, 8])
assert(get([3, 5, 8], 99, -1) == -1)
assert(find([3, 5, 8], 8) == 2)
assert(count([3, 5, 3, 8, 3], 3) == 3)
assert(not contains([3, 5, 8, 13], 21))
assert(contains("secure Fyr", "Fyr"))
assert(slice("secure Fyr", 0, 6) == "secure")
assert(get("Fyr", 1, "?") == "y")
assert(find("secure Fyr", "Fyr") == 7)
assert(count("secure Fyr secure", "secure") == 2)
assert(is_empty(""))
assert([1, 2, 3] == [1, 2, 3])
assert(total == 55, "total should match the counted loop")
```

Run them with:

```sh
fyr test examples
```

When `fyr check` or `fyr test` receives a directory, it recursively finds `.fyr` files.

The bootstrap version of Fyr is intentionally small. Each chapter of this book should track real language behavior as the compiler grows.

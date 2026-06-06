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

Mutation is explicit:

```fyr
var total = 0
var i = 1

while i <= 10:
    total = total + i
    i = i + 1

print(total)
```

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
let more_values = values + [21]
let empty: [i64] = []
print(sum(more_values))
print(len(empty))
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
assert([1, 2, 3] == [1, 2, 3])
assert(total == 55, "total should match the counted loop")
```

Run them with:

```sh
fyr test examples/assertions.fyr
```

The bootstrap version of Fyr is intentionally small. Each chapter of this book should track real language behavior as the compiler grows.

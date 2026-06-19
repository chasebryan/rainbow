# Rainbow

small language, early compiler.

trying to make code fast, safe, and easy to read.

```sh
cargo run -p rainbow -- run examples/hello.rain
cargo run -p rainbow -- check examples
cargo run -p rainbow -- test examples
```

install it:

```sh
cargo install --path crates/rainbow --force
rainbow doctor
```

what works:

- repl
- parser
- interpreter
- type checker
- formatter
- imports
- tests
- projects
- functions
- structs
- enums
- arrays
- strings
- nullable values

```rainbow
fn bracket(value: str, left: str, right: str) -> str:
    return left + value + right

let label = "  Rainbow  " then trim then lower then bracket("[", "]")
print(label)
```

projects use `rainbow.toml`.

```toml
name = "hello"
main = "src/main.rain"
```

still rough. moving fast.

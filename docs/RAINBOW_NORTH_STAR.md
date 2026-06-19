# Rainbow North Star

Rainbow is not a rename project. Rainbow is a language project with an intentionally unreasonable bar:

- faster than C in the benchmark classes where a modern compiler can use stronger semantic information than C exposes
- safer than Rust in the sense that safe Rainbow should cover memory, data races, effects, capabilities, nil, arithmetic, and dependency authority with fewer escape hatches
- as readable as Ruby for everyday application code
- as good as the best ideas from the rest of programming language history without becoming visually noisy

Those lines are targets, not marketing claims. Every target needs a compiler mechanism, a benchmark, a safety check, a syntax rule, or a standard-library design rule behind it.

## Faster Than C

Rainbow should not mean "C with nicer syntax." It should use information that C normally cannot rely on:

- no undefined behavior in safe code
- explicit aliasing and ownership information
- precise mutability
- bounds-check elimination from proven loop shapes
- layout control where performance requires it
- automatic vectorization opportunities from safe iteration APIs
- specialization and monomorphization for hot generic paths
- profile-guided and whole-program optimization modes
- predictable allocation and escape analysis
- no hidden VM, reflection tax, or mandatory garbage collector in systems mode

The benchmark rule is strict: each performance claim must name the workload, the C baseline, the optimization mode, and the reason Rainbow can equal or beat it. "Faster than C" becomes real only when it is backed by checked benchmarks.

Initial benchmark classes:

- tight numeric loops
- string scanning and transformation
- array/map/filter/reduce-style data movement
- parser/tokenizer workloads
- small HTTP request routing
- task scheduling and message passing
- FFI boundary overhead
- allocation-heavy transformations with escape analysis

## Safer Than Rust

Rust raised the floor for mainstream systems safety. Rainbow should keep that floor and add broader default safety:

- no null dereferences in safe code
- no use-after-free in safe code
- no data races in safe code
- no unchecked integer overflow by default
- no untracked effectful IO in pure contexts
- no ambient filesystem, process, network, clock, or environment authority without capability flow
- no dependency scripts with hidden authority by default
- no implicit lossy numeric conversions
- no unhandled absence when a value is nullable
- no unsafe block that can spread authority without an explicit boundary

Unsafe Rainbow should be rarer, narrower, and easier to audit than unsafe Rust. The goal is not to shame low-level code; the goal is to make the boundary unmistakable.

## Readable As Ruby

Ruby's gift is not looseness. Its gift is humane, flowing code. Rainbow should take the flow and keep static rigor:

- names should carry meaning before punctuation does
- common code should read top to bottom
- transformations should compose left to right with `|>`
- blocks should stay visually light
- APIs should prefer one obvious expression over a matrix of clever overloads
- formatter output is the language's visual contract
- error messages should explain the compiler's model, not just reject syntax

Readable does not mean dynamic. Rainbow can be statically checked and still feel smooth.

## Best-Of Sources

Rainbow should deliberately absorb these strengths:

- C: layout, ABI reach, predictable cost
- Zig: explicit allocation, comptime discipline, cross-compilation pragmatism
- Rust: ownership, enums, pattern matching, fearless refactoring
- Ruby: readable blocks, fluent data transformation, humane APIs
- Python: approachable scripts, batteries-included workflows, teaching clarity
- ML/F#/Haskell: algebraic data, inference, exhaustive matching, expression-oriented design
- Lisp/Racket: language extension and compile-time abstraction
- Smalltalk: live exploration and object-message clarity
- Erlang/Elixir: supervision, message passing, resilient systems
- Go: fast builds, simple deployment, straightforward concurrency ergonomics
- Swift/Kotlin/TypeScript: modern application ergonomics, optionality, tooling, editor feedback
- Julia/R: numerical and data-oriented expressiveness
- SQL: declarative set thinking and optimizer-visible intent
- Shell/Make: composable tools and honest automation

The rule is synthesis, not collage. If two inherited ideas fight, Rainbow chooses the one that preserves local reasoning, safety, and visual calm.

## Product Modes

Rainbow should support these modes as one language:

- systems binary
- application service
- script
- package/library
- data analysis notebook or REPL session
- concurrent worker
- embedded or FFI component
- teaching language

The same source should be able to grow from script to package to native executable without a rewrite.

## Current Compiler Commitments

The bootstrap is still interpreted, but it already establishes several non-negotiable habits:

- executable examples stay in the repo
- syntax changes must pass parser, formatter, typechecker, evaluator, docs, and examples
- checks run through `cargo test`, `cargo clippy`, `fyr check`, `fyr test`, and formatter validation
- source examples favor readable names and explicit safety over cleverness

The new `|>` operator is a small example of the larger rule: borrow a good idea, make it statically checked, keep it visually quiet, and prove it with a running example.

## What Comes Next

The next hard work should be compiler-facing:

1. Introduce a native IR with explicit value ownership and effect metadata.
2. Add generics and specialization without making common signatures noisy.
3. Define an ownership/capability model before adding broad IO.
4. Build benchmark fixtures beside equivalent C/Rust/Ruby/Python implementations.
5. Add comparative examples where each feature is checked against source-language idioms.
6. Keep adding small, real language features that prove the philosophy in code.

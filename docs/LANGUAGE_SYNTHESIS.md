# Rainbow Language Synthesis

Rainbow studies and absorbs the best ideas from many programming traditions without becoming a collage. The hard engineering bar for speed, safety, and readability is tracked in [RAINBOW_NORTH_STAR.md](RAINBOW_NORTH_STAR.md).

The core standard is beauty. In Rainbow, beauty means:

- the syntax shows program structure before decoration
- the type system helps model reality without making simple code heavy
- the runtime behavior is explicit, predictable, and fast
- powerful features compose instead of fighting each other
- tools make the correct path feel natural
- diagnostics teach the programmer what the language knows
- claims about speed or safety are benchmarked, checked, and traceable to a compiler mechanism

## Design Law

Rainbow does not take a feature because another language has it. Rainbow takes a feature only when it satisfies all four tests:

1. It makes common code clearer.
2. It makes serious code safer or faster.
3. It composes with the rest of the language.
4. It can be explained with a small example.

If a feature is powerful but visually noisy, Rainbow should find the quieter form. If a feature is elegant but hides cost, Rainbow should surface the cost. If a feature is expressive but weakens local reasoning, Rainbow should constrain it.

## The Spectrum

### Red: Native Power

From C, C++, Zig, Odin, and Rust:

- predictable native execution
- explicit data layout where needed
- direct interoperability with operating systems and C ABIs
- zero-cost abstractions when the abstraction is honest
- safe defaults with explicit escape hatches

Rainbow should let programmers write high-level code that compiles down to obvious machine behavior, then drop into lower-level control when the work demands it.

### Orange: Safety

From Rust, Swift, Ada, Pony, and modern static analysis:

- memory safety without garbage collection as the only answer
- data-race prevention
- checked arithmetic and bounds by default
- null absence represented in the type system
- explicit unsafe blocks that are narrow and auditable
- capability-style authority for IO, concurrency, and effects

Rainbow should make invalid states hard to express and dangerous operations easy to find.

### Yellow: Readability

From Python, Ruby, Lua, and Smalltalk:

- low-noise syntax for everyday programs
- indentation that makes structure visible
- readable names over punctuation tricks
- a REPL that feels like the front door
- libraries shaped around human workflows

Rainbow should feel simple before it feels clever.

### Green: Data And Modeling

From ML, Haskell, F#, Scala, Elm, and TypeScript:

- algebraic data types
- pattern matching
- exhaustive checks
- inference that removes repetition without removing intent
- generics with strong constraints
- structural and nominal modeling where each fits

Rainbow should make a domain model read like the domain itself.

### Blue: Concurrency And Distribution

From Erlang, Elixir, Go, Pony, Rust, and CSP systems:

- lightweight tasks
- structured concurrency
- cancellation and timeouts as first-class design concerns
- message passing where sharing would create risk
- supervised long-running work
- data-race freedom across task boundaries

Rainbow should make concurrent code calm enough to review.

### Indigo: Extensibility

From Lisp, Racket, Julia, Nim, and Elixir:

- hygienic macro-like extension when ordinary functions are not enough
- compile-time execution with explicit boundaries
- domain-specific notation without parser chaos
- reflection that helps tooling rather than bypassing safety

Rainbow should be extensible, but extension should preserve readability for people who did not write the extension.

### Violet: Tooling And Experience

From TypeScript, Rust, Go, Swift, Kotlin, Cargo, Deno, and modern editors:

- one excellent command
- formatter as part of the language contract
- fast check/test/build loops
- precise errors with source context
- package and documentation flows that do not require ceremony
- language-server quality from the beginning

Rainbow should make the toolchain feel like part of the language, not a pile of scripts beside it.

## First-Class Modes

Rainbow should support several programming modes without splitting into separate dialects:

- Script: one file, fast feedback, clear IO
- App: project manifest, packages, tests, formatter, build artifacts
- Systems: explicit memory, layout, FFI, native backend
- Data: tables, streams, numerics, plotting hooks, reproducible analysis
- Concurrent service: structured tasks, cancellation, supervision, observability
- Interactive notebook/REPL: live exploration that can become durable source
- Library: stable public APIs, docs, examples, compatibility checks

The language should let a prototype grow into a package, a package into a service, and a service into a native executable without rewriting the mental model.

## Current Bootstrap

The current compiler is still small, but it already carries several Rainbow commitments:

- typed function signatures
- readable indented blocks
- checked integer and floating-point operations
- explicit nullable `T?` and `nil`
- safe `??` recovery
- scoped `if let` binding
- nominal structs
- unit and payload enums
- exhaustive enum `match`
- enum-pattern `if let`
- homogeneous arrays
- checked string and array indexing
- recursive imports inside a project boundary
- a REPL, formatter, checker, test runner, and build command

This is enough to keep the project runnable while the design grows.

## Near-Term Language Lanes

The next language work should build toward Rainbow without losing the working bootstrap:

1. Tool identity: keep `fyr` as an alias, then introduce a `rainbow` command, `rainbow.toml`, and `.rainbow` or shorter source extension only when the migration path is clear.
2. Native performance: introduce IR, benchmark fixtures, ownership metadata, bounds-check elimination, and specialization.
3. Type system: add generics, richer inference, effect boundaries, and better branch narrowing.
4. Ownership and safety: define safe value movement, borrowing or regions, capability flow, and explicit unsafe boundaries.
5. Standard library: design beautiful primitives for text, paths, time, collections, IO, data, concurrency, and errors.
6. Comparative examples: add parallel examples that show one idea expressed in C, Rust, Ruby, Python, TypeScript, Haskell/ML, SQL, and Rainbow, then use those examples to refine the syntax.
7. Interactive experience: make the REPL, formatter, diagnostics, project scaffold, and book feel like one product.

## Comparative Example Shape

Every canonical example should eventually include:

- the human problem being solved
- the idiomatic expression in several source languages
- the Rainbow expression
- why Rainbow chose that shape
- what the compiler can prove
- what the runtime guarantees

The point is not translation for its own sake. The point is disciplined comparison that lets Rainbow keep the best ideas and reject accidental complexity.

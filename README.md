<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <b>codegen-lang</b>
    <br>
    <sub><sup>CODE GENERATION</sup></sub>
</h1>

<div align="center">
    <a href="https://crates.io/crates/codegen-lang"><img alt="Crates.io" src="https://img.shields.io/crates/v/codegen-lang"></a>
    <a href="https://crates.io/crates/codegen-lang"><img alt="Downloads" src="https://img.shields.io/crates/d/codegen-lang?color=%230099ff"></a>
    <a href="https://docs.rs/codegen-lang"><img alt="docs.rs" src="https://img.shields.io/docsrs/codegen-lang"></a>
    <a href="https://github.com/jamesgober/codegen-lang/actions"><img alt="CI" src="https://github.com/jamesgober/codegen-lang/actions/workflows/ci.yml/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
</div>

<br>

<div align="left">
    <p>
        codegen-lang is the CODE-tier crate: A backend abstraction that lowers IR to LLVM, Cranelift, or bytecode targets. Part of the -lang language-construction family; see _strategy/LANG_COLLECTION.md for the master plan.
    </p>
    <br>
    <hr>
    <p>
        <strong>MSRV is 1.85+</strong> (Rust 2024 edition).
    </p>
    <blockquote>
        <strong>Status: stable.</strong> The public API is frozen as of <code>1.0.0</code> and follows Semantic Versioning, with no breaking changes before <code>2.0</code>. See <a href="./docs/API.md#semver-promise"><code>docs/API.md</code></a> for the SemVer promise and <a href="./CHANGELOG.md"><code>CHANGELOG.md</code></a>.
    </blockquote>
</div>

<hr>
<br>

## Overview

A compiler's middle end produces a function in [SSA form](https://en.wikipedia.org/wiki/Static_single-assignment_form); the backend turns that control-flow graph into a linear stream of instructions a machine can run. codegen-lang draws the line between the two.

[`Backend`](./docs/API.md#backend) is the trait a target implements. [`Bytecode`](./docs/API.md#bytecode) is the target shipped here: a small, register-based bytecode that is enough to inspect, serialize, and run generated code without pulling in a native code generator. A backend built on LLVM or Cranelift slots in behind the same trait, producing its own output type.

The input is an [`ir_lang::Function`](https://docs.rs/ir-lang). Each IR value is given its own virtual register, each basic block becomes a label in a flat op stream, and a block's parameters are filled by `move` ops emitted on the edges that target it — the bytecode's stand-in for an SSA phi. The function is validated before lowering, so a backend only ever sees well-formed SSA.

<br>
<hr>
<br>

## Installation

```toml
[dependencies]
codegen-lang = "1"
ir-lang = "1"
```

Or from the terminal:

```bash
cargo add codegen-lang ir-lang
```

<br>

## Quick Start

```rust
use codegen_lang::{compile, Op};
use ir_lang::{Builder, BinOp, Type};

// Build `fn double(x: int) -> int { x + x }` with the IR builder.
let mut b = Builder::new("double", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let sum = b.bin(BinOp::Add, x, x);
b.ret(Some(sum));

// Lower it to bytecode.
let program = compile(&b.finish()).expect("double is well-formed");

assert_eq!(program.name(), "double");
assert!(matches!(program.ops()[0], Op::Bin { op: BinOp::Add, .. }));
assert!(matches!(program.ops()[1], Op::Return { value: Some(_) }));

// The Display impl is a readable disassembly:
//   double(r0) regs=2
//   L0:
//       r1 = add r0, r0
//       ret r1
println!("{program}");
```

<br>
<hr>
<br>

## Lowering model

The bytecode is register-based rather than stack-based. The pieces map onto the IR directly:

| IR construct | Lowers to |
|---|---|
| A value (instruction result or block parameter) | A virtual [`Reg`](./docs/API.md#reg)ister, numbered by the value's index |
| An instruction (`iconst`, `bin`, `un`, …) | One [`Op`](./docs/API.md#op) writing its result register |
| A basic block | A [`Label`](./docs/API.md#label) at the op where the block begins |
| A block argument on an edge | A `move` into the target block's parameter register |
| A two-way branch | A `jump_unless` plus two exclusive arms |

Because the source and destination registers of an edge are disjoint in SSA, the argument copies never interfere — no parallel-move scheduling is required. The result is a flat `&[Op]` that an interpreter or a further pass can walk with no indirection.

### A branch and a loop

```rust
use codegen_lang::{compile, Op};
use ir_lang::{Builder, BinOp, Type, UnOp};

// fn abs(x: int) -> int { if x < 0 { -x } else { x } }
let mut b = Builder::new("abs", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let join = b.create_block(&[Type::Int]);
let neg_blk = b.create_block(&[]);
let pos_blk = b.create_block(&[]);

let zero = b.iconst(0);
let is_neg = b.bin(BinOp::Lt, x, zero);
b.branch(is_neg, neg_blk, &[], pos_blk, &[]);

b.switch_to(neg_blk);
let negated = b.un(UnOp::Neg, x);
b.jump(join, &[negated]);

b.switch_to(pos_blk);
b.jump(join, &[x]);

b.switch_to(join);
let result = b.block_params(join)[0];
b.ret(Some(result));

let program = compile(&b.finish()).expect("abs is well-formed");

// The branch became a conditional skip; the winner reaches the join via a move.
assert!(program.ops().iter().any(|op| matches!(op, Op::JumpUnless { .. })));
assert!(program.ops().iter().any(|op| matches!(op, Op::Move { .. })));
```

<br>
<hr>
<br>

## API Overview

For a complete reference with examples, see [`docs/API.md`](./docs/API.md).

- [`Backend`](./docs/API.md#backend) — the trait a code generator implements; lowers a `Function` to a target `Output`.
- [`Bytecode`](./docs/API.md#bytecode) — the reference backend, emitting a flat [`Program`](./docs/API.md#program).
- [`compile`](./docs/API.md#compile) — the shortcut for lowering with the bytecode backend.
- [`Program`](./docs/API.md#program) — the lowered function: name, parameter registers, register count, op stream, and a disassembly `Display`.
- [`Op`](./docs/API.md#op) — one bytecode instruction; the closed set the op stream is made of.
- [`Reg`](./docs/API.md#reg) / [`Label`](./docs/API.md#label) — a register and a jump target.
- [`Const`](./docs/API.md#const) — a constant operand (`Int` / `Float` / `Bool`).
- [`CodegenError`](./docs/API.md#codegenerror) — the reason a function could not be lowered.
- `BinOp` / `UnOp` — re-exported from `ir-lang` so the operations carried by `Op::Bin` and `Op::Un` can be matched without naming `ir-lang` directly.

### Error handling

`compile` validates its input with [`Function::validate`](https://docs.rs/ir-lang) before lowering. If the function is not well-formed, it returns [`CodegenError::InvalidIr`](./docs/API.md#codegenerror) carrying the precise reason; a backend never emits a program that is wrong in a way the IR already forbids.

```rust
use codegen_lang::{compile, CodegenError};
use ir_lang::{Builder, Type};

// A function whose entry block never receives a terminator is not well-formed.
let func = Builder::new("f", &[], Type::Unit).finish();
match compile(&func) {
    Err(CodegenError::InvalidIr(reason)) => assert!(reason.to_string().contains("terminator")),
    other => panic!("expected an InvalidIr error, got {other:?}"),
}
```

<br>
<hr>
<br>

## Performance

Performance is a hard constraint, not an afterthought (see [`REPS.md`](./REPS.md)). Lowering is a single linear pass over the function: each value maps to one op, each edge adds a small fixed number of moves and a jump, and the op vector is preallocated. The cost is linear in function size.

Latest local Criterion means (`cargo bench --bench bench --all-features`, Windows x86_64 / WSL2, Rust stable, release build). `compile` includes the up-front validation pass:

| Function shape | Size | `compile` time |
|---|---:|---:|
| Straight-line arithmetic | 16 instructions | ~0.23 µs |
| Straight-line arithmetic | 256 instructions | ~2.2 µs |
| Straight-line arithmetic | 4096 instructions | ~31 µs |
| Diamond branches | 512 branches | ~121 µs |
| Countdown loop | header + back-edge | ~0.35 µs |

Numbers vary by CPU and environment; run the benches on your hardware for trends.

<br>
<hr>
<br>

## Configuration

### Feature Flags

| Feature | Default | Description |
|---|---|---|
| `std` | on | Links the standard library. Without it the crate is `#![no_std]` and needs only `alloc`. |
| `serde` | off | Derives `Serialize` / `Deserialize` for `Program` and its parts, for caching or moving a compiled program between tools. |

```toml
# no_std build:
codegen-lang = { version = "1", default-features = false }

# with serialization:
codegen-lang = { version = "1", features = ["serde"] }
```

<br>

## Examples

Two runnable examples live in [`examples/`](./examples):

```bash
# Lower double / abs / a loop and print each one's disassembly.
cargo run --example disassemble

# Compile a function, walk its op stream, and see a malformed function rejected.
cargo run --example inspect
```

<br>

## Testing

```bash
# Unit, integration (workflow), and doc tests
cargo test --all-features

# Property tests only
cargo test --all-features --test properties

# Lints and formatting
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings

# Benchmarks (Criterion)
cargo bench --bench bench --all-features
```

The integration suite in [`tests/workflow.rs`](./tests/workflow.rs) compiles and runs representative functions — `double`, `abs`, `max`, a countdown loop — through a reference interpreter and checks the results. The property tests generate random straight-line functions and check that compiling and running matches an independent evaluation, and that every emitted program is structurally sound.

<br>

## Cross-Platform Support

The crate is pure, dependency-light Rust with no platform-specific code, and is tested on Linux, macOS, and Windows (x86_64) through the CI matrix on stable and the 1.85 MSRV.

<hr>
<br>

## Contributing

Engineering standards for this crate are the [Rust Efficiency &amp; Performance Standards](./REPS.md); the current scope and plan are in [`dev/ROADMAP.md`](./dev/ROADMAP.md). Before a PR: `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test --all-features` must be clean.

<br>

<div id="license">
    <h2>License</h2>
    <p>Licensed under either of</p>
    <ul>
        <li><b>Apache License, Version 2.0</b> &mdash; <a href="./LICENSE-APACHE">LICENSE-APACHE</a></li>
        <li><b>MIT License</b> &mdash; <a href="./LICENSE-MIT">LICENSE-MIT</a></li>
    </ul>
    <p>at your option.</p>
</div>

<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>James Gober <me@jamesgober.com>.</strong></sup>
</div>

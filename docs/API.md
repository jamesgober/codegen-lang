<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>codegen-lang</b><br>
    <sub><sup>API REFERENCE</sup></sub>
</h1>
<div align="center">
    <sup>
        <a href="../README.md" title="Project Home"><b>HOME</b></a>
        <span>&nbsp;│&nbsp;</span>
        <span>API</span>
        <span>&nbsp;│&nbsp;</span>
        <a href="../dev/ROADMAP.md" title="Roadmap"><b>ROADMAP</b></a>
    </sup>
</div>
<br>

> **Status: pre-1.0.** The public surface is being designed across the 0.x series and frozen at `1.0.0`. Until then it may change between minor versions. See [`dev/ROADMAP.md`](../dev/ROADMAP.md).

A backend abstraction that lowers the [`ir-lang`](https://docs.rs/ir-lang) intermediate representation to a concrete target. The shipped target is a small, register-based bytecode.

<br>

## Table of Contents

- [Installation](#installation)
- [Lowering model](#lowering-model)
- [Public API](#public-api)
  - [`compile`](#compile)
  - [`Backend`](#backend)
  - [`Bytecode`](#bytecode)
  - [`Program`](#program)
  - [`Op`](#op)
  - [`Reg`](#reg)
  - [`Label`](#label)
  - [`Const`](#const)
  - [`CodegenError`](#codegenerror)
  - [`BinOp` & `UnOp` (re-exports)](#binop--unop)
- [Feature flags](#feature-flags)
- [SemVer](#semver)

<br>
<hr>
<br>

## Installation

```toml
[dependencies]
codegen-lang = "0.2"
ir-lang = "1"
```

```bash
cargo add codegen-lang ir-lang
```

The crate is `#![no_std]`-capable: build with `default-features = false` to drop the standard library and depend only on `alloc`.

<br>
<hr>
<br>

## Lowering model

A backend reads an [`ir_lang::Function`](https://docs.rs/ir-lang) — a control-flow graph of basic blocks in SSA form — and lays it out as a linear program. The bytecode backend's mapping:

- **Each value** the function defines (an instruction result or a block parameter) is held in its own virtual [`Reg`](#reg)ister, numbered by the value's index. The total is [`Program::register_count`](#program).
- **Each instruction** becomes one [`Op`](#op) writing its result register.
- **Each basic block** becomes a [`Label`](#label) at the op where the block begins. The entry block is always `L0`, the first op.
- **A block argument** on a control-flow edge becomes a `move` into the target block's parameter register, emitted on the edge — the stand-in for an SSA phi.
- **A two-way branch** becomes a [`JumpUnless`](#op) plus two exclusive arms, so the argument moves of the arm not taken never run.

The function is checked with [`Function::validate`](https://docs.rs/ir-lang) before lowering, so the emitted program is faithful to well-formed SSA and a malformed function is rejected with a [`CodegenError`](#codegenerror) rather than miscompiled.

<br>
<hr>
<br>

## Public API

### `compile`

```rust
pub fn compile(func: &ir_lang::Function) -> Result<Program, CodegenError>
```

Lowers a function to bytecode with the default [`Bytecode`](#bytecode) backend. This is the shortcut for the common case where the bytecode target is the one you want; reach for the [`Backend`](#backend) trait when you need to be generic over backends.

**Parameters**

- `func` — the function to lower, in SSA form, as produced by `ir_lang::Builder`. It is validated before lowering.

**Returns**

- `Ok(Program)` — the lowered [`Program`](#program).
- `Err(CodegenError::InvalidIr(_))` — `func` did not pass `Function::validate`; the wrapped reason names the offending block or value.

**Examples**

Lower a straight-line function:

```rust
use codegen_lang::compile;
use ir_lang::{Builder, BinOp, Type};

// fn triple(x: int) -> int { x + x + x }
let mut b = Builder::new("triple", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let two_x = b.bin(BinOp::Add, x, x);
let three_x = b.bin(BinOp::Add, two_x, x);
b.ret(Some(three_x));

let program = compile(&b.finish()).expect("triple is well-formed");
assert_eq!(program.name(), "triple");
assert_eq!(program.register_count(), 3);
```

Handle a rejection:

```rust
use codegen_lang::{compile, CodegenError};
use ir_lang::{Builder, Type};

// Declares an int return but returns nothing.
let mut b = Builder::new("bad", &[], Type::Int);
b.ret(None);

assert!(matches!(compile(&b.finish()), Err(CodegenError::InvalidIr(_))));
```

<br>

### `Backend`

```rust
pub trait Backend {
    type Output;
    fn compile(&self, func: &ir_lang::Function) -> Result<Self::Output, CodegenError>;
}
```

The abstraction the crate is built around: a code generator that lowers a function to a concrete target representation. The [`Bytecode`](#bytecode) backend shipped here emits a [`Program`](#program); a backend layered on a native code generator (LLVM, Cranelift) would produce that generator's own module type as its `Output`. A backend holds no per-call state, so one instance compiles many functions.

**Associated types**

- `Output` — the representation this backend emits.

**Methods**

- `compile(&self, func)` — lowers `func` to `Output`. By convention a backend validates its input first and returns [`CodegenError::InvalidIr`](#codegenerror) on failure, so lowering proper only runs on well-formed SSA.

**Examples**

Compile through the trait:

```rust
use codegen_lang::{Backend, Bytecode};
use ir_lang::{Builder, Type};

let mut b = Builder::new("noop", &[], Type::Unit);
b.ret(None);

let program = Bytecode.compile(&b.finish()).expect("noop is well-formed");
assert_eq!(program.name(), "noop");
```

Write a function generic over any backend:

```rust
use codegen_lang::{Backend, Bytecode};
use ir_lang::{Builder, Type};

fn count_outputs<B: Backend>(backend: &B, func: &ir_lang::Function) -> Result<B::Output, codegen_lang::CodegenError> {
    backend.compile(func)
}

let mut b = Builder::new("f", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
b.ret(Some(x));

let program = count_outputs(&Bytecode, &b.finish()).unwrap();
assert_eq!(program.register_count(), 1);
```

<br>

### `Bytecode`

```rust
pub struct Bytecode;
```

The reference backend. It validates the function, then lowers each block in order to a linear op stream — small enough to read, serialize, and execute, which makes it the natural target for testing a front-end's output before a native backend exists. A zero-sized type with no configuration; construct it directly or with `Default`.

Implements [`Backend`](#backend) with `Output = Program`.

**Examples**

```rust
use codegen_lang::{Backend, Bytecode};
use ir_lang::{Builder, BinOp, Type};

// fn inc(x: int) -> int { x + 1 }
let mut b = Builder::new("inc", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let one = b.iconst(1);
let sum = b.bin(BinOp::Add, x, one);
b.ret(Some(sum));

let program = Bytecode.compile(&b.finish()).expect("inc is well-formed");
assert_eq!(program.register_count(), 3); // x, the constant, the sum
```

One instance compiles many functions:

```rust
use codegen_lang::{Backend, Bytecode};
use ir_lang::{Builder, Type};

let backend = Bytecode::default();
for name in ["a", "b", "c"] {
    let mut b = Builder::new(name, &[], Type::Unit);
    b.ret(None);
    assert_eq!(backend.compile(&b.finish()).unwrap().name(), name);
}
```

<br>

### `Program`

```rust
pub struct Program { /* private fields */ }
```

A lowered function: a flat bytecode program ready to be inspected, serialized, or run. It owns the function's name, the registers holding its parameters, a count of every register it uses, and the op stream. Control-flow ops refer to positions in that stream through [`Label`](#label)s. Execution begins at the first op, the entry block.

The `Display` implementation renders the program as a readable disassembly.

**Methods**

| Method | Returns | Description |
|---|---|---|
| `name()` | `&str` | The function's name. |
| `params()` | `&[Reg]` | The registers holding the parameters, in declaration order. An interpreter writes the call arguments into these before running. |
| `register_count()` | `u32` | The number of registers; valid register numbers are `0..register_count`. |
| `ops()` | `&[Op]` | The ops, in execution order. |
| `len()` | `usize` | The number of ops. |
| `is_empty()` | `bool` | Whether there are no ops. A program lowered from a valid function is never empty. |
| `label_offset(label)` | `Option<usize>` | The op index a label points at, or `None` if the label is not part of this program. |
| `entry()` | `Label` | The entry label, `L0`, where execution begins. |

**Examples**

Inspect the structure of a compiled function:

```rust
use codegen_lang::{compile, Op};
use ir_lang::{Builder, BinOp, Type};

let mut b = Builder::new("double", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let sum = b.bin(BinOp::Add, x, x);
b.ret(Some(sum));
let program = compile(&b.finish()).unwrap();

assert_eq!(program.name(), "double");
assert_eq!(program.params().len(), 1);
assert_eq!(program.register_count(), 2);
assert_eq!(program.len(), 2);
assert!(matches!(program.ops().last(), Some(Op::Return { value: Some(_) })));
```

Read the disassembly:

```rust
use codegen_lang::compile;
use ir_lang::{Builder, BinOp, Type};

let mut b = Builder::new("double", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let sum = b.bin(BinOp::Add, x, x);
b.ret(Some(sum));
let text = compile(&b.finish()).unwrap().to_string();

assert!(text.starts_with("double(r0) regs=2"));
assert!(text.contains("r1 = add r0, r0"));
assert!(text.contains("ret r1"));
```

Resolve a label to an op offset:

```rust
use codegen_lang::compile;
use ir_lang::{Builder, Type};

let mut b = Builder::new("f", &[], Type::Unit);
b.ret(None);
let program = compile(&b.finish()).unwrap();

assert_eq!(program.label_offset(program.entry()), Some(0));
```

<br>

### `Op`

```rust
pub enum Op {
    Const { dst: Reg, value: Const },
    Bin { op: BinOp, dst: Reg, lhs: Reg, rhs: Reg },
    Un { op: UnOp, dst: Reg, src: Reg },
    Move { dst: Reg, src: Reg },
    Jump { target: Label },
    JumpUnless { cond: Reg, target: Label },
    Return { value: Option<Reg> },
}
```

One bytecode instruction; the closed set a [`Program`](#program)'s op stream is made of. Every variant is `Copy`, so an op stream is a flat slice with no indirection.

**Variants**

| Variant | Meaning |
|---|---|
| `Const { dst, value }` | Load a [constant](#const) into `dst`. |
| `Bin { op, dst, lhs, rhs }` | `dst = lhs <op> rhs`, reusing the IR's `BinOp`. |
| `Un { op, dst, src }` | `dst = <op> src`, reusing the IR's `UnOp`. |
| `Move { dst, src }` | `dst = src`. Emitted on a control-flow edge to pass a block argument into a parameter register. |
| `Jump { target }` | Jump unconditionally to `target`. |
| `JumpUnless { cond, target }` | Jump to `target` when `cond` is `false`; otherwise fall through. |
| `Return { value }` | Return, optionally yielding the value in a register. |

Each variant's `Display` is one disassembly line: `r2 = add r0, r1`, `jump L1`, `jump_unless r0, L2`, `ret r1`, and so on.

**Examples**

Match on the ops of a compiled function:

```rust
use codegen_lang::{compile, Op};
use ir_lang::{Builder, BinOp, Type, UnOp};

// fn f(x: int) -> int { -(x * x) }
let mut b = Builder::new("f", &[Type::Int], Type::Int);
let x = b.block_params(b.entry())[0];
let sq = b.bin(BinOp::Mul, x, x);
let neg = b.un(UnOp::Neg, sq);
b.ret(Some(neg));
let program = compile(&b.finish()).unwrap();

let mul = program.ops().iter().filter(|op| matches!(op, Op::Bin { op: BinOp::Mul, .. })).count();
let neg = program.ops().iter().filter(|op| matches!(op, Op::Un { op: UnOp::Neg, .. })).count();
assert_eq!((mul, neg), (1, 1));
```

Build an op directly and render it:

```rust
use codegen_lang::{Const, Op, Reg};

let load = Op::Const { dst: Reg(0), value: Const::Int(1) };
assert_eq!(load.to_string(), "r0 = const 1");
```

<br>

### `Reg`

```rust
pub struct Reg(pub u32);
```

A virtual register: a numbered slot that holds one value while a program runs. Registers are dense from zero; [`Program::register_count`](#program) is one past the highest in use. Displays as `r{n}`.

**Examples**

```rust
use codegen_lang::Reg;

let r = Reg(2);
assert_eq!(r.0, 2);
assert_eq!(r.to_string(), "r2");
```

<br>

### `Label`

```rust
pub struct Label(pub u32);
```

A jump target: a position in a program's op stream that a control-flow op transfers to. Each basic block becomes a label numbered by block index, so the entry block is `L0`. A two-way branch adds one internal label for its second arm. Resolve a label to an op index with [`Program::label_offset`](#program). Displays as `L{n}`.

**Examples**

```rust
use codegen_lang::Label;

assert_eq!(Label(0).to_string(), "L0");
assert_eq!(Label(3).to_string(), "L3");
```

<br>

### `Const`

```rust
pub enum Const {
    Int(i64),
    Float(f64),
    Bool(bool),
}
```

A constant operand loaded by [`Op::Const`](#op). The three cases mirror the IR's three constant instructions and carry the same payloads, so a constant is reproduced exactly.

**Examples**

```rust
use codegen_lang::Const;

assert_eq!(Const::Int(-7).to_string(), "-7");
assert_eq!(Const::Bool(true).to_string(), "true");
```

<br>

### `CodegenError`

```rust
#[non_exhaustive]
pub enum CodegenError {
    InvalidIr(ir_lang::ValidationError),
}
```

The reason a [`Backend`](#backend) could not lower a function. Lowering needs well-formed SSA; a backend checks that up front and refuses to emit code for input that fails, rather than producing a program that is wrong in a way the IR already forbids.

`CodegenError` implements `Display`, `std::error::Error` (with `source` set to the underlying reason), and `From<ir_lang::ValidationError>`. The enum is `#[non_exhaustive]`, so a `match` on it must include a wildcard arm.

**Variants**

- `InvalidIr(ValidationError)` — the function did not pass `Function::validate`. The wrapped [`ValidationError`](https://docs.rs/ir-lang) names the offending block or value and explains the violation.

**Examples**

Inspect the reason:

```rust
use codegen_lang::{compile, CodegenError};
use ir_lang::{Builder, Type};

let func = Builder::new("f", &[], Type::Unit).finish(); // no terminator
match compile(&func) {
    Err(CodegenError::InvalidIr(reason)) => {
        assert!(reason.to_string().contains("terminator"));
    }
    other => panic!("expected InvalidIr, got {other:?}"),
}
```

Use it as a `std::error::Error`:

```rust
use codegen_lang::compile;
use ir_lang::{Builder, Type};
use std::error::Error;

let func = Builder::new("f", &[], Type::Int).finish(); // missing return value
let err = compile(&func).unwrap_err();
assert!(err.source().is_some());
```

<br>

### `BinOp` & `UnOp`

Re-exported from `ir-lang` so the operations carried by [`Op::Bin`](#op) and [`Op::Un`](#op) can be named and matched without depending on `ir-lang` directly. They are the IR's own operation enums, reused unchanged:

- `BinOp` — `Add`, `Sub`, `Mul`, `Div`, `Eq`, `Ne`, `Lt`, `Le`, `Gt`, `Ge`, `And`, `Or`.
- `UnOp` — `Neg`, `Not`.

```rust
use codegen_lang::{BinOp, Op, Reg, UnOp};

let add = Op::Bin { op: BinOp::Add, dst: Reg(2), lhs: Reg(0), rhs: Reg(1) };
let not = Op::Un { op: UnOp::Not, dst: Reg(1), src: Reg(0) };
assert_eq!(add.to_string(), "r2 = add r0, r1");
assert_eq!(not.to_string(), "r1 = not r0");
```

<br>
<hr>
<br>

## Feature flags

| Feature | Default | Description |
|---|---|---|
| `std` | on | Links the standard library. Without it the crate is `#![no_std]` and needs only `alloc`. |
| `serde` | off | Derives `Serialize` / `Deserialize` for `Program`, `Op`, `Reg`, `Label`, `Const`, and `CodegenError`. |

Both features forward to the matching `ir-lang` feature, so a serialized program round-trips the IR types it carries.

<br>

## SemVer

This crate is **pre-1.0**. The public surface above may change between minor (`0.x`) versions while it is being designed; it will be frozen and given a SemVer stability promise at `1.0.0`. Pin a `0.2` range and consult the [`CHANGELOG`](../CHANGELOG.md) when upgrading.

<br>
<hr>

<sub>Copyright &copy; 2026 <strong>James Gober</strong>.</sub>

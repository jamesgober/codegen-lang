//! # codegen_lang
//!
//! A backend abstraction that turns the [`ir-lang`](ir_lang) intermediate representation
//! into code for a concrete target.
//!
//! A compiler's middle end hands the backend a function in SSA form; the backend lays
//! that control-flow graph out as a linear stream of instructions a machine can run.
//! This crate draws the line between the two. [`Backend`] is the trait a target
//! implements, and [`Bytecode`] is the target shipped here: a small, register-based
//! bytecode that is enough to inspect, serialize, and run generated code without pulling
//! in a native code generator. A backend built on LLVM or Cranelift slots in behind the
//! same trait, producing its own [`Output`](Backend::Output).
//!
//! ## Lowering model
//!
//! Each IR value is given its own virtual [`Reg`]ister, so an [`Op`] names its operands
//! and result by register rather than by a position on a stack. Each basic block becomes
//! a [`Label`] in a flat [op stream](Program::ops). A block's parameters are filled by
//! [`Move`](Op::Move) ops emitted on the edges that target it — the bytecode's stand-in
//! for an SSA phi node, which is how a value crosses a control-flow join. The input is
//! checked with [`Function::validate`](ir_lang::Function::validate) before lowering, so
//! a backend only ever sees well-formed SSA and the op stream it returns is faithful to
//! the function it came from.
//!
//! ## Example
//!
//! Lower `fn double(x: int) -> int { x + x }` and read the result back:
//!
//! ```
//! use codegen_lang::{compile, BinOp, Op};
//! use ir_lang::{Builder, Type};
//!
//! let mut b = Builder::new("double", &[Type::Int], Type::Int);
//! let x = b.block_params(b.entry())[0];
//! let sum = b.bin(BinOp::Add, x, x);
//! b.ret(Some(sum));
//!
//! let program = compile(&b.finish()).expect("double is well-formed");
//! assert_eq!(program.name(), "double");
//!
//! // One add, then a return of its result.
//! assert!(matches!(program.ops()[0], Op::Bin { op: BinOp::Add, .. }));
//! assert!(matches!(program.ops()[1], Op::Return { value: Some(_) }));
//! ```
//!
//! ## Features
//!
//! - `std` (default) — links the standard library. Without it the crate is `#![no_std]`
//!   and needs only `alloc`.
//! - `serde` — derives `Serialize` and `Deserialize` for [`Program`] and its parts, so a
//!   compiled program can be cached or moved between tools.
//!
//! ## Stability
//!
//! Pre-1.0: the public surface is still being designed and may change between minor
//! versions until it is frozen at `1.0.0`. The current scope and plan are recorded in
//! `docs/API.md` and `dev/ROADMAP.md`.

#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]
#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::unreachable,
    clippy::dbg_macro,
    clippy::print_stdout,
    clippy::print_stderr
)]

extern crate alloc;

mod backend;
mod error;
mod lower;
mod program;

pub use backend::{Backend, Bytecode, compile};
pub use error::CodegenError;
pub use program::{Const, Label, Op, Program, Reg};

// Re-exported so the operations carried by [`Op::Bin`] and [`Op::Un`] can be named and
// matched without depending on `ir-lang` directly. They are the IR's own operation
// enums, reused unchanged.
pub use ir_lang::{BinOp, UnOp};

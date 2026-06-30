//! Inspect a compiled program, and see how a malformed function is rejected.
//!
//! Where [`disassemble`](disassemble.rs) prints the whole program, this walks the op
//! stream programmatically — the way a further pass or a target emitter would consume
//! it — and then shows the error path.
//!
//! Run it with:
//!
//! ```text
//! cargo run --example inspect
//! ```

use codegen_lang::{Op, compile};
use ir_lang::{BinOp, Builder, Type};

fn main() {
    // fn quadratic(x: int) -> int { x * x + x } — a few values and a return.
    let mut b = Builder::new("quadratic", &[Type::Int], Type::Int);
    let x = b.block_params(b.entry())[0];
    let sq = b.bin(BinOp::Mul, x, x);
    let plus_x = b.bin(BinOp::Add, sq, x);
    b.ret(Some(plus_x));

    let program = compile(&b.finish()).expect("quadratic is well-formed");

    println!("function : {}", program.name());
    println!("params   : {}", program.params().len());
    println!("registers: {}", program.register_count());
    println!("ops      : {}", program.len());

    // Count the arithmetic and control-flow ops.
    let mut arithmetic = 0;
    let mut control = 0;
    for op in program.ops() {
        match op {
            Op::Const { .. } | Op::Bin { .. } | Op::Un { .. } | Op::Move { .. } => arithmetic += 1,
            Op::Jump { .. } | Op::JumpUnless { .. } | Op::Return { .. } => control += 1,
        }
    }
    println!("  {arithmetic} arithmetic op(s), {control} control-flow op(s)");

    // The error path: a function declared to return an int but returning nothing is not
    // well-formed, and the backend reports exactly why rather than emitting bad code.
    let broken = Builder::new("broken", &[], Type::Int).finish();
    match compile(&broken) {
        Ok(_) => println!("broken   : unexpectedly compiled"),
        Err(err) => println!("broken   : rejected — {err}"),
    }
}

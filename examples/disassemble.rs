//! Lower a few functions and print the bytecode each one compiles to.
//!
//! This is the shortest way to see what the backend produces: build a function with the
//! IR builder, lower it with [`codegen_lang::compile`], and print the resulting
//! [`Program`], whose `Display` is a readable disassembly.
//!
//! Run it with:
//!
//! ```text
//! cargo run --example disassemble
//! ```

use codegen_lang::compile;
use ir_lang::{BinOp, Builder, Function, Type, UnOp};

/// `fn double(x: int) -> int { x + x }` — straight-line code.
fn double() -> Function {
    let mut b = Builder::new("double", &[Type::Int], Type::Int);
    let x = b.block_params(b.entry())[0];
    let sum = b.bin(BinOp::Add, x, x);
    b.ret(Some(sum));
    b.finish()
}

/// `fn abs(x: int) -> int { if x < 0 { -x } else { x } }` — a two-way branch joining
/// through a block parameter.
fn abs() -> Function {
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
    b.finish()
}

/// `fn sum_to_zero(n: int) -> int { ... }` — a loop whose header carries two block
/// parameters across the back-edge.
fn sum_to_zero() -> Function {
    let mut b = Builder::new("sum_to_zero", &[Type::Int], Type::Int);
    let n0 = b.block_params(b.entry())[0];
    let header = b.create_block(&[Type::Int, Type::Int]);
    let body = b.create_block(&[]);
    let exit = b.create_block(&[]);

    let zero = b.iconst(0);
    b.jump(header, &[n0, zero]);
    b.switch_to(header);
    let n = b.block_params(header)[0];
    let acc = b.block_params(header)[1];
    let z = b.iconst(0);
    let more = b.bin(BinOp::Gt, n, z);
    b.branch(more, body, &[], exit, &[]);
    b.switch_to(body);
    let acc2 = b.bin(BinOp::Add, acc, n);
    let one = b.iconst(1);
    let n2 = b.bin(BinOp::Sub, n, one);
    b.jump(header, &[n2, acc2]);
    b.switch_to(exit);
    b.ret(Some(acc));
    b.finish()
}

fn main() {
    for func in [double(), abs(), sum_to_zero()] {
        match compile(&func) {
            Ok(program) => {
                println!("{program}");
            }
            Err(err) => println!("{}: {err}", func.name()),
        }
    }
}

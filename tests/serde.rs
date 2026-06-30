//! Round-trip tests for the `serde` feature: a compiled program and a codegen error
//! survive serialization and deserialization unchanged.

#![cfg(feature = "serde")]

use codegen_lang::{CodegenError, Program, compile};
use ir_lang::{BinOp, Builder, Type, UnOp};

#[test]
fn test_program_round_trips_through_json() {
    // A function with a branch and block-parameter edges, so the round trip covers every
    // op kind: constants, a binary op, a unary op, moves, jumps, and a return.
    let mut b = Builder::new("compute", &[Type::Int, Type::Int], Type::Int);
    let a = b.block_params(b.entry())[0];
    let c = b.block_params(b.entry())[1];
    let join = b.create_block(&[Type::Int]);
    let then_blk = b.create_block(&[]);
    let else_blk = b.create_block(&[]);

    let cond = b.bin(BinOp::Ge, a, c);
    b.branch(cond, then_blk, &[], else_blk, &[]);
    b.switch_to(then_blk);
    let neg = b.un(UnOp::Neg, a);
    b.jump(join, &[neg]);
    b.switch_to(else_blk);
    b.jump(join, &[c]);
    b.switch_to(join);
    let result = b.block_params(join)[0];
    b.ret(Some(result));

    let original = compile(&b.finish()).expect("compute is well-formed");

    let json = serde_json::to_string(&original).expect("serialization succeeds");
    let restored: Program = serde_json::from_str(&json).expect("deserialization succeeds");

    assert_eq!(restored, original);
    // The disassembly is identical, which exercises the deserialized labels and ops.
    assert_eq!(restored.to_string(), original.to_string());
}

#[test]
fn test_codegen_error_round_trips_through_json() {
    let err: CodegenError = compile(&Builder::new("g", &[], Type::Int).finish())
        .expect_err("a non-unit function with no return is invalid");

    let json = serde_json::to_string(&err).expect("serialization succeeds");
    let restored: CodegenError = serde_json::from_str(&json).expect("deserialization succeeds");

    assert_eq!(restored, err);
}

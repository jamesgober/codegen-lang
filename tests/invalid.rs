//! A backend must refuse IR that is not well-formed rather than emit a wrong program.
//!
//! These build functions the IR builder is happy to assemble but
//! [`Function::validate`](ir_lang::Function::validate) rejects, and confirm
//! [`codegen_lang::compile`] surfaces that as [`CodegenError::InvalidIr`] carrying the
//! specific reason.

use codegen_lang::{CodegenError, compile};
use ir_lang::{BinOp, Builder, Type, ValidationError};

fn reason(func: ir_lang::Function) -> ValidationError {
    match compile(&func) {
        Err(CodegenError::InvalidIr(reason)) => reason,
        Err(other) => panic!("expected an InvalidIr error, got {other:?}"),
        Ok(_) => panic!("a malformed function must not compile"),
    }
}

#[test]
fn test_unterminated_block_is_rejected() {
    // The entry block never gets a terminator.
    let func = Builder::new("f", &[], Type::Unit).finish();
    assert!(matches!(
        reason(func),
        ValidationError::MissingTerminator { .. }
    ));
}

#[test]
fn test_missing_return_value_is_rejected() {
    // Returns nothing from a function declared to return int.
    let mut b = Builder::new("f", &[], Type::Int);
    b.ret(None);
    assert!(matches!(
        reason(b.finish()),
        ValidationError::ReturnValueExpected { .. }
    ));
}

#[test]
fn test_type_mismatched_operands_are_rejected() {
    // int + bool.
    let mut b = Builder::new("f", &[Type::Int, Type::Bool], Type::Int);
    let x = b.block_params(b.entry())[0];
    let flag = b.block_params(b.entry())[1];
    let bad = b.bin(BinOp::Add, x, flag);
    b.ret(Some(bad));
    assert!(matches!(
        reason(b.finish()),
        ValidationError::TypeMismatch { .. }
    ));
}

#[test]
fn test_branch_to_entry_is_rejected() {
    // A jump back onto the entry block, which must have no predecessors.
    let mut b = Builder::new("f", &[], Type::Unit);
    let entry = b.entry();
    b.jump(entry, &[]);
    assert!(matches!(
        reason(b.finish()),
        ValidationError::EntryBranchTarget { .. }
    ));
}

#[test]
fn test_argument_count_mismatch_is_rejected() {
    // Jumps to a one-parameter block with no arguments.
    let mut b = Builder::new("f", &[], Type::Int);
    let exit = b.create_block(&[Type::Int]);
    b.jump(exit, &[]);
    b.switch_to(exit);
    let p = b.block_params(exit)[0];
    b.ret(Some(p));
    assert!(matches!(
        reason(b.finish()),
        ValidationError::ArgCountMismatch { .. }
    ));
}

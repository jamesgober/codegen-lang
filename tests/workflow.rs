//! End-to-end codegen workflow tests.
//!
//! Each test walks the full path a front-end takes: build a function with the IR
//! builder, lower it with [`codegen_lang::compile`], then both inspect the emitted
//! program and run it through the reference interpreter to confirm it computes what the
//! source program means.

mod support;

use codegen_lang::{Op, compile};
use ir_lang::{BinOp, Builder, Type, UnOp};
use support::{Value, run};

#[test]
fn test_double_compiles_and_runs() {
    // fn double(x: int) -> int { x + x }
    let mut b = Builder::new("double", &[Type::Int], Type::Int);
    let x = b.block_params(b.entry())[0];
    let sum = b.bin(BinOp::Add, x, x);
    b.ret(Some(sum));
    let program = compile(&b.finish()).expect("double is well-formed");

    assert_eq!(program.name(), "double");
    assert_eq!(program.register_count(), 2);
    assert!(matches!(
        program.ops().last(),
        Some(Op::Return { value: Some(_) })
    ));

    assert_eq!(run(&program, &[Value::Int(21)]), Value::Int(42));
    assert_eq!(run(&program, &[Value::Int(-5)]), Value::Int(-10));
}

#[test]
fn test_abs_with_a_branch_runs_both_arms() {
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

    // The branch lowers to a conditional skip plus the two arms.
    assert!(
        program
            .ops()
            .iter()
            .any(|op| matches!(op, Op::JumpUnless { .. }))
    );

    assert_eq!(run(&program, &[Value::Int(-7)]), Value::Int(7));
    assert_eq!(run(&program, &[Value::Int(7)]), Value::Int(7));
    assert_eq!(run(&program, &[Value::Int(0)]), Value::Int(0));
}

#[test]
fn test_max_diamond_passes_the_winner_through_a_block_parameter() {
    // fn max(a: int, b: int) -> int { if a < b { b } else { a } }
    let mut f = Builder::new("max", &[Type::Int, Type::Int], Type::Int);
    let a = f.block_params(f.entry())[0];
    let c = f.block_params(f.entry())[1];
    let join = f.create_block(&[Type::Int]);
    let then_blk = f.create_block(&[]);
    let else_blk = f.create_block(&[]);

    let cond = f.bin(BinOp::Lt, a, c);
    f.branch(cond, then_blk, &[], else_blk, &[]);
    f.switch_to(then_blk);
    f.jump(join, &[c]);
    f.switch_to(else_blk);
    f.jump(join, &[a]);
    f.switch_to(join);
    let r = f.block_params(join)[0];
    f.ret(Some(r));

    let program = compile(&f.finish()).expect("max is well-formed");

    // The winner reaches the join through a move on each incoming edge.
    assert!(program.ops().iter().any(|op| matches!(op, Op::Move { .. })));

    assert_eq!(
        run(&program, &[Value::Int(3), Value::Int(9)]),
        Value::Int(9)
    );
    assert_eq!(
        run(&program, &[Value::Int(9), Value::Int(3)]),
        Value::Int(9)
    );
    assert_eq!(
        run(&program, &[Value::Int(4), Value::Int(4)]),
        Value::Int(4)
    );
}

#[test]
fn test_countdown_loop_runs_to_completion() {
    // fn sum_to_zero(n: int) -> int { let mut acc = 0; while n > 0 { acc += n; n -= 1; } acc }
    //
    // Modeled as a loop header carrying (n, acc) as block parameters.
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

    let program = compile(&b.finish()).expect("sum_to_zero is well-formed");

    // 5 + 4 + 3 + 2 + 1 = 15; a non-positive start sums to 0.
    assert_eq!(run(&program, &[Value::Int(5)]), Value::Int(15));
    assert_eq!(run(&program, &[Value::Int(1)]), Value::Int(1));
    assert_eq!(run(&program, &[Value::Int(0)]), Value::Int(0));
    assert_eq!(run(&program, &[Value::Int(-3)]), Value::Int(0));
}

#[test]
fn test_float_and_bool_paths_lower_and_run() {
    // fn cmp(a: float, b: float) -> bool { !(a < b) }
    let mut f = Builder::new("not_less", &[Type::Float, Type::Float], Type::Bool);
    let a = f.block_params(f.entry())[0];
    let c = f.block_params(f.entry())[1];
    let less = f.bin(BinOp::Lt, a, c);
    let not_less = f.un(UnOp::Not, less);
    f.ret(Some(not_less));
    let program = compile(&f.finish()).expect("not_less is well-formed");

    assert_eq!(
        run(&program, &[Value::Float(1.0), Value::Float(2.0)]),
        Value::Bool(false),
    );
    assert_eq!(
        run(&program, &[Value::Float(2.0), Value::Float(1.0)]),
        Value::Bool(true),
    );
}

#[test]
fn test_unit_function_returns_unit() {
    let mut b = Builder::new("noop", &[], Type::Unit);
    b.ret(None);
    let program = compile(&b.finish()).expect("noop is well-formed");
    assert_eq!(run(&program, &[]), Value::Unit);
}

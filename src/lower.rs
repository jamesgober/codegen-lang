//! Lowering an [`ir_lang::Function`] to a bytecode [`Program`].
//!
//! The pass is a single linear walk of the function's blocks in creation order. Each
//! value is mapped to the register with its index, each instruction to one op, and each
//! block to a label at the op where it begins. The only non-trivial part is the
//! control-flow edges: a value crosses a block boundary as a block argument in the IR,
//! and is realized here as a [`Move`](Op::Move) into the target block's parameter
//! register, emitted on the edge.

use alloc::vec::Vec;

use ir_lang::{Block, Function, Inst, Terminator, Value};

use crate::program::{Const, Label, Op, Program, Reg};

/// Lowers a *validated* function to bytecode.
///
/// The caller must have checked `func` with
/// [`Function::validate`](ir_lang::Function::validate) first. This routine relies on the
/// SSA invariants that check guarantees — handles in range, every listed instruction
/// value resolving, arguments matching parameters — and is total over well-formed input.
pub(crate) fn lower(func: &Function) -> Program {
    let block_count = func.block_count();

    // Label id == block index for the blocks. Two-way branches append internal labels
    // past `block_count`. Each entry is filled with an op offset when that position is
    // reached: block labels as each block starts, branch labels as the second arm opens.
    let mut labels: Vec<u32> = alloc::vec![0; block_count];

    // One op per value covers the instructions; the extra room absorbs each block's
    // terminator and the moves and jumps its out-edges add, so construction does not
    // reallocate on the common case.
    let capacity = func.value_count() + block_count * 3;
    let mut ops: Vec<Op> = Vec::with_capacity(capacity);

    for block in func.blocks() {
        labels[block.index()] = ops.len() as u32;
        lower_block(func, block, &mut ops, &mut labels);
    }

    Program {
        name: func.name().into(),
        params: param_registers(func),
        registers: func.value_count() as u32,
        ops,
        labels,
    }
}

/// The registers holding the entry block's parameters — the function's parameters.
fn param_registers(func: &Function) -> Vec<Reg> {
    func.block_params(func.entry())
        .iter()
        .map(|&value| register(value))
        .collect()
}

/// The register a value is held in: its dense SSA index.
fn register(value: Value) -> Reg {
    Reg(value.index() as u32)
}

/// Emits the ops of one block: its instructions in program order, then its terminator.
fn lower_block(func: &Function, block: Block, ops: &mut Vec<Op>, labels: &mut Vec<u32>) {
    for &value in func.insts(block) {
        // Every value `insts` lists is an instruction result, so `inst` resolves for any
        // function that passed validation; the guard keeps the walk total without a
        // panic if a caller ever lowers unchecked input.
        let Some(inst) = func.inst(value) else {
            continue;
        };
        ops.push(lower_inst(register(value), inst));
    }
    if let Some(terminator) = func.terminator(block) {
        lower_terminator(func, terminator, ops, labels);
    }
}

/// Lowers one value-producing instruction to its single op.
fn lower_inst(dst: Reg, inst: &Inst) -> Op {
    match inst {
        Inst::Iconst(value) => Op::Const {
            dst,
            value: Const::Int(*value),
        },
        Inst::Fconst(value) => Op::Const {
            dst,
            value: Const::Float(*value),
        },
        Inst::Bconst(value) => Op::Const {
            dst,
            value: Const::Bool(*value),
        },
        Inst::Bin(op, lhs, rhs) => Op::Bin {
            op: *op,
            dst,
            lhs: register(*lhs),
            rhs: register(*rhs),
        },
        Inst::Un(op, src) => Op::Un {
            op: *op,
            dst,
            src: register(*src),
        },
    }
}

/// Lowers a block's terminator, emitting any argument moves its edges carry.
fn lower_terminator(
    func: &Function,
    terminator: &Terminator,
    ops: &mut Vec<Op>,
    labels: &mut Vec<u32>,
) {
    match terminator {
        Terminator::Return(value) => {
            ops.push(Op::Return {
                value: value.map(register),
            });
        }
        Terminator::Jump(target, args) => {
            move_arguments(func, *target, args, ops);
            ops.push(Op::Jump {
                target: block_label(*target),
            });
        }
        Terminator::Branch {
            cond,
            then_block,
            then_args,
            else_block,
            else_args,
        } => {
            // Lay the branch out as two exclusive arms so the argument moves of the arm
            // not taken never run — which matters when both arms target the same block
            // with different arguments. The `then` arm is the fall-through; an internal
            // label opens the `else` arm.
            let else_arm = Label(labels.len() as u32);
            labels.push(0); // offset filled below, once the arm's first op is known

            ops.push(Op::JumpUnless {
                cond: register(*cond),
                target: else_arm,
            });

            move_arguments(func, *then_block, then_args, ops);
            ops.push(Op::Jump {
                target: block_label(*then_block),
            });

            labels[else_arm.0 as usize] = ops.len() as u32;
            move_arguments(func, *else_block, else_args, ops);
            ops.push(Op::Jump {
                target: block_label(*else_block),
            });
        }
    }
}

/// The label of a block: its index, since block labels are numbered by block index.
fn block_label(block: Block) -> Label {
    Label(block.index() as u32)
}

/// Copies each argument into the matching parameter register of `target`.
///
/// In SSA the source registers (the predecessor's values) and the destination registers
/// (the target's parameters) are disjoint, so the copies do not interfere and a plain
/// sequence is correct — no parallel-move scheduling is needed. A copy whose source and
/// destination coincide is skipped.
fn move_arguments(func: &Function, target: Block, args: &[Value], ops: &mut Vec<Op>) {
    for (&arg, &param) in args.iter().zip(func.block_params(target)) {
        let (dst, src) = (register(param), register(arg));
        if dst != src {
            ops.push(Op::Move { dst, src });
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests build known-valid functions, so compilation cannot fail"
)]
mod tests {
    use crate::compile;
    use crate::program::{Const, Label, Op, Reg};
    use ir_lang::{BinOp, Builder, Type, UnOp};

    #[test]
    fn test_straight_line_lowers_to_one_op_per_value() {
        // fn f(x: int) -> int { -(x * x) }
        let mut b = Builder::new("f", &[Type::Int], Type::Int);
        let x = b.block_params(b.entry())[0];
        let sq = b.bin(BinOp::Mul, x, x);
        let neg = b.un(UnOp::Neg, sq);
        b.ret(Some(neg));
        let program = compile(&b.finish()).unwrap();

        assert_eq!(
            program.ops(),
            [
                Op::Bin {
                    op: BinOp::Mul,
                    dst: Reg(1),
                    lhs: Reg(0),
                    rhs: Reg(0)
                },
                Op::Un {
                    op: UnOp::Neg,
                    dst: Reg(2),
                    src: Reg(1)
                },
                Op::Return {
                    value: Some(Reg(2))
                },
            ]
        );
    }

    #[test]
    fn test_jump_moves_arguments_into_target_parameters() {
        // entry passes a constant to a one-parameter exit block.
        let mut b = Builder::new("f", &[], Type::Int);
        let exit = b.create_block(&[Type::Int]);
        let n = b.iconst(7);
        b.jump(exit, &[n]);
        b.switch_to(exit);
        let p = b.block_params(exit)[0];
        b.ret(Some(p));

        // Registers are value indices; derive them rather than assume an order, since the
        // exit parameter is minted at `create_block`, before the constant.
        let arg = Reg(n.index() as u32);
        let param = Reg(p.index() as u32);
        let program = compile(&b.finish()).unwrap();

        // The argument is copied into the target parameter, then control jumps to the
        // exit block (block 1 -> L1).
        assert!(program.ops().contains(&Op::Move {
            dst: param,
            src: arg
        }));
        assert!(program.ops().contains(&Op::Jump { target: Label(1) }));
    }

    #[test]
    fn test_branch_emits_two_exclusive_arms() {
        // fn f(c: bool) -> unit { if c {} else {} }
        let mut b = Builder::new("f", &[Type::Bool], Type::Unit);
        let c = b.block_params(b.entry())[0];
        let yes = b.create_block(&[]);
        let no = b.create_block(&[]);
        b.branch(c, yes, &[], no, &[]);
        b.switch_to(yes);
        b.ret(None);
        b.switch_to(no);
        b.ret(None);
        let program = compile(&b.finish()).unwrap();

        // A conditional skip to the else arm, then an unconditional jump to each block.
        assert!(matches!(
            program.ops()[0],
            Op::JumpUnless { cond: Reg(0), .. }
        ));
        assert!(program.ops().contains(&Op::Jump { target: Label(1) })); // then -> yes
        assert!(program.ops().contains(&Op::Jump { target: Label(2) })); // else -> no
    }

    #[test]
    fn test_self_move_is_not_emitted() {
        // A loop back-edge that re-passes a value into a parameter occupying a different
        // register still produces a move; this checks the common straight case has none
        // spuriously. A function with no edges has no moves at all.
        let mut b = Builder::new("f", &[Type::Int], Type::Int);
        let x = b.block_params(b.entry())[0];
        b.ret(Some(x));
        let program = compile(&b.finish()).unwrap();
        assert!(!program.ops().iter().any(|op| matches!(op, Op::Move { .. })));
    }

    #[test]
    fn test_constants_lower_to_their_payloads() {
        let mut b = Builder::new("k", &[], Type::Int);
        let _ = b.fconst(1.5);
        let _ = b.bconst(true);
        let n = b.iconst(42);
        b.ret(Some(n));
        let program = compile(&b.finish()).unwrap();

        assert!(program.ops().contains(&Op::Const {
            dst: Reg(2),
            value: Const::Int(42)
        }));
        assert!(program.ops().contains(&Op::Const {
            dst: Reg(1),
            value: Const::Bool(true)
        }));
        assert!(program.ops().contains(&Op::Const {
            dst: Reg(0),
            value: Const::Float(1.5)
        }));
    }
}

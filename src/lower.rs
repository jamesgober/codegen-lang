//! Lowering an [`ir_lang::Function`] to a bytecode [`Program`].
//!
//! The pass is a single linear walk of the function's blocks in creation order. Each
//! value is mapped to the register with its index, each instruction to one op, and each
//! block to a label at the op where it begins. The only non-trivial part is the
//! control-flow edges: a value crosses a block boundary as a block argument in the IR,
//! and is realized here as [`Move`](Op::Move)s into the target block's parameter
//! registers, emitted on the edge. The arguments of one edge are a *parallel* copy, so
//! their moves are ordered by a [`Mover`] (see the `moves` module for why that matters).

use alloc::vec::Vec;

use ir_lang::{Block, Function, Inst, Terminator, Value};

use crate::moves::{Mover, narrow};
use crate::program::{Const, Label, Op, Program, Reg};

/// Lowers a *validated* function to bytecode.
///
/// The caller must have checked `func` with
/// [`Function::validate`](ir_lang::Function::validate) first. This routine relies on the
/// SSA invariants that check guarantees — handles in range, every listed instruction
/// value resolving, arguments matching parameters — and is total over well-formed input.
///
/// Every `u32` in the output (register numbers, the register count, labels, op offsets)
/// is produced by [`narrow`], whose doc comment states the bound: it is exact for any
/// function the bytecode format can describe, and never panics or wraps beyond it.
pub(crate) fn lower(func: &Function) -> Program {
    let block_count = func.block_count();
    let value_count = func.value_count();

    // Label id == block index for the blocks. Two-way branches append internal labels
    // past `block_count`. Each entry is filled with an op offset when that position is
    // reached: block labels as each block starts, branch labels as the second arm opens.
    let mut labels: Vec<u32> = alloc::vec![0; block_count];

    // One op per value covers the instructions; the extra room absorbs each block's
    // terminator and the moves and jumps its out-edges add, so construction does not
    // reallocate on the common case.
    let capacity = value_count + block_count * 3;
    let mut ops: Vec<Op> = Vec::with_capacity(capacity);

    // The cycle-breaking scratch register sits just past the value registers, so it can
    // never alias a value. It joins the register file only if some edge used it, which
    // keeps `register_count` exact: one past the highest register the ops name.
    let mut mover = Mover::new(value_count, Reg(narrow(value_count)));

    for block in func.blocks() {
        labels[block.index()] = narrow(ops.len());
        lower_block(func, block, &mut ops, &mut labels, &mut mover);
    }

    let registers = value_count + usize::from(mover.scratch_used());
    Program {
        name: func.name().into(),
        params: param_registers(func),
        registers: narrow(registers),
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

/// The register a value is held in: its dense SSA index. ir-lang numbers values with
/// `u32` handles, so the index always fits.
fn register(value: Value) -> Reg {
    Reg(narrow(value.index()))
}

/// Emits the ops of one block: its instructions in program order, then its terminator.
fn lower_block(
    func: &Function,
    block: Block,
    ops: &mut Vec<Op>,
    labels: &mut Vec<u32>,
    mover: &mut Mover,
) {
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
        lower_terminator(func, terminator, ops, labels, mover);
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
    mover: &mut Mover,
) {
    match terminator {
        Terminator::Return(value) => {
            ops.push(Op::Return {
                value: value.map(register),
            });
        }
        Terminator::Jump(target, args) => {
            move_arguments(func, *target, args, ops, mover);
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
            // label opens the `else` arm. Each arm is its own parallel copy, scheduled
            // independently. `jump_unless` reads the condition before either arm's moves
            // run, so an arm overwriting the condition's register is harmless.
            let else_slot = labels.len();
            let else_arm = Label(narrow(else_slot));
            labels.push(0); // offset filled below, once the arm's first op is known

            ops.push(Op::JumpUnless {
                cond: register(*cond),
                target: else_arm,
            });

            move_arguments(func, *then_block, then_args, ops, mover);
            ops.push(Op::Jump {
                target: block_label(*then_block),
            });

            labels[else_slot] = narrow(ops.len());
            move_arguments(func, *else_block, else_args, ops, mover);
            ops.push(Op::Jump {
                target: block_label(*else_block),
            });
        }
    }
}

/// The label of a block: its index, since block labels are numbered by block index.
/// ir-lang numbers blocks with `u32` handles, so the index always fits.
fn block_label(block: Block) -> Label {
    Label(narrow(block.index()))
}

/// Copies each argument into the matching parameter register of `target`, as one
/// parallel assignment.
///
/// The sources and destinations of an edge are *not* disjoint in general: a loop back
/// edge may pass the target's own parameters permuted (`jump H(b, a)` inside `H(a, b)`),
/// shifted (`jump H(a2, a)`), or duplicated (`jump H(a, a)`). Emitting the copies in
/// argument order would then overwrite a parameter before it is read. The [`Mover`]
/// orders them so no pending source is overwritten and breaks each cycle with the
/// function's scratch register. A copy whose source and destination coincide is
/// skipped.
fn move_arguments(
    func: &Function,
    target: Block,
    args: &[Value],
    ops: &mut Vec<Op>,
    mover: &mut Mover,
) {
    let pairs = args
        .iter()
        .zip(func.block_params(target))
        .map(|(&arg, &param)| (register(param), register(arg)));
    mover.edge(pairs, ops);
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests build known-valid functions, so compilation cannot fail"
)]
mod tests {
    use crate::compile;
    use crate::program::{Const, Label, Op, Reg};
    use alloc::vec::Vec;
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
    fn test_function_without_edges_has_no_moves() {
        let mut b = Builder::new("f", &[Type::Int], Type::Int);
        let x = b.block_params(b.entry())[0];
        b.ret(Some(x));
        let program = compile(&b.finish()).unwrap();
        assert!(!program.ops().iter().any(|op| matches!(op, Op::Move { .. })));
    }

    /// `H(a, b): branch k, H(b, a), exit` — the back edge of the 1.0.0 miscompile, with
    /// nothing else on it. Returns the function and the header's parameter registers.
    fn swap_loop() -> (ir_lang::Function, Reg, Reg) {
        let mut b = Builder::new("swap", &[Type::Int, Type::Int, Type::Bool], Type::Int);
        let p = b.block_params(b.entry()).to_vec();
        let header = b.create_block(&[Type::Int, Type::Int]);
        let exit = b.create_block(&[]);
        b.jump(header, &[p[0], p[1]]);
        b.switch_to(header);
        let h = b.block_params(header).to_vec();
        b.branch(p[2], header, &[h[1], h[0]], exit, &[]);
        b.switch_to(exit);
        b.ret(Some(h[0]));
        let reg = |v: ir_lang::Value| Reg(v.index() as u32);
        let (a, c) = (reg(h[0]), reg(h[1]));
        (b.finish(), a, c)
    }

    #[test]
    fn test_back_edge_swap_goes_through_the_scratch_register() {
        let (func, a, c) = swap_loop();
        let scratch = Reg(func.value_count() as u32);
        let program = compile(&func).unwrap();

        // The then-arm (the back edge) is the three moves of a swap through the scratch
        // register — never `a = b; b = a`.
        let arm: Vec<Op> = program
            .ops()
            .iter()
            .skip_while(|op| !matches!(op, Op::JumpUnless { .. }))
            .skip(1)
            .take(3)
            .copied()
            .collect();
        assert_eq!(
            arm,
            [
                Op::Move {
                    dst: scratch,
                    src: c
                },
                Op::Move { dst: c, src: a },
                Op::Move {
                    dst: a,
                    src: scratch
                },
            ]
        );
        assert_eq!(program.register_count(), scratch.0 + 1);
    }

    #[test]
    fn test_programs_without_cycles_keep_the_value_register_count() {
        let mut b = Builder::new("f", &[Type::Int], Type::Int);
        let x = b.block_params(b.entry())[0];
        let exit = b.create_block(&[Type::Int, Type::Int]);
        b.jump(exit, &[x, x]);
        b.switch_to(exit);
        let p = b.block_params(exit)[1];
        b.ret(Some(p));
        let func = b.finish();
        let program = compile(&func).unwrap();
        assert_eq!(program.register_count() as usize, func.value_count());
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

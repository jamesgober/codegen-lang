//! A direct interpreter for `ir-lang` functions: the oracle the CFG tests check codegen
//! against.
//!
//! It runs the IR itself, never the lowered bytecode, so it shares no code with the
//! backend's move scheduling. Block arguments are bound as the IR defines them: a true
//! parallel assignment, with every argument read before any parameter is written. Values
//! live in an environment indexed by value, overwritten each time their definition runs
//! again (a loop header's parameters on every iteration), which is the dynamic meaning of
//! SSA that the register machine must reproduce. Arithmetic shares the bytecode
//! interpreter's operator semantics (wrapping integers), so the two can only disagree
//! about *data flow*, which is exactly what the tests probe.

use ir_lang::{Function, Terminator, Value as IrValue};

use super::{Value, binary, unary};

/// Evaluates `func` on `args` and returns what it returns.
///
/// `fuel` bounds the number of blocks entered, so a generator bug that builds a
/// non-terminating loop fails the test instead of hanging it. Panics on a function that
/// is not well-formed (the tests only evaluate validated functions) or that runs out of
/// fuel.
#[must_use]
pub fn eval(func: &Function, args: &[Value], fuel: usize) -> Value {
    let mut env: Vec<Option<Value>> = vec![None; func.value_count()];
    let entry = func.entry();
    let params = func.block_params(entry);
    assert_eq!(params.len(), args.len(), "argument count mismatch");
    for (&param, &arg) in params.iter().zip(args) {
        env[param.index()] = Some(arg);
    }

    let mut block = entry;
    let mut remaining = fuel;
    loop {
        remaining = remaining
            .checked_sub(1)
            .unwrap_or_else(|| panic!("evaluation ran out of fuel ({fuel} blocks)"));

        for &value in func.insts(block) {
            let inst = func.inst(value).expect("a listed instruction resolves");
            let result = match *inst {
                ir_lang::Inst::Iconst(v) => Value::Int(v),
                ir_lang::Inst::Fconst(v) => Value::Float(v),
                ir_lang::Inst::Bconst(v) => Value::Bool(v),
                ir_lang::Inst::Bin(op, lhs, rhs) => binary(op, read(&env, lhs), read(&env, rhs)),
                ir_lang::Inst::Un(op, src) => unary(op, read(&env, src)),
            };
            env[value.index()] = Some(result);
        }

        match func
            .terminator(block)
            .expect("a validated block is terminated")
        {
            Terminator::Return(value) => {
                return value.map_or(Value::Unit, |v| read(&env, v));
            }
            Terminator::Jump(target, args) => {
                enter(func, &mut env, *target, args);
                block = *target;
            }
            Terminator::Branch {
                cond,
                then_block,
                then_args,
                else_block,
                else_args,
            } => {
                let (target, args) = if read(&env, *cond).boolean() {
                    (*then_block, then_args)
                } else {
                    (*else_block, else_args)
                };
                enter(func, &mut env, target, args);
                block = target;
            }
        }
    }
}

/// Binds `args` to `target`'s parameters as one parallel assignment.
fn enter(func: &Function, env: &mut [Option<Value>], target: ir_lang::Block, args: &[IrValue]) {
    let incoming: Vec<Value> = args.iter().map(|&a| read(env, a)).collect();
    for (&param, value) in func.block_params(target).iter().zip(incoming) {
        env[param.index()] = Some(value);
    }
}

fn read(env: &[Option<Value>], value: IrValue) -> Value {
    env[value.index()].unwrap_or_else(|| panic!("{value} read before it was defined"))
}

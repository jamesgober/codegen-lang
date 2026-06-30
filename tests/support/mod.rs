//! A small reference interpreter for the bytecode, shared by the integration tests.
//!
//! It is the independent oracle the tests check codegen against: build a function, lower
//! it with the crate, run the result here, and compare the answer to what the source
//! program should compute. Integer arithmetic wraps on overflow so a run is always
//! defined; this is a test artifact, not a statement about the bytecode's semantics.

#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::panic,
    reason = "test-support code: each test crate uses a subset, and a bad value should fail loudly"
)]

use codegen_lang::{BinOp, Const, Op, Program, Reg, UnOp};

/// A runtime value flowing through the registers as a program runs.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Value {
    /// A signed integer.
    Int(i64),
    /// A floating-point number.
    Float(f64),
    /// A boolean.
    Bool(bool),
    /// The absence of a value.
    Unit,
}

impl Value {
    fn int(self) -> i64 {
        match self {
            Value::Int(v) => v,
            other => panic!("expected an int, found {other:?}"),
        }
    }

    fn float(self) -> f64 {
        match self {
            Value::Float(v) => v,
            other => panic!("expected a float, found {other:?}"),
        }
    }

    fn boolean(self) -> bool {
        match self {
            Value::Bool(v) => v,
            other => panic!("expected a bool, found {other:?}"),
        }
    }
}

/// Runs `program` with `args` bound to its parameter registers and returns the value it
/// returns. Panics if the program is malformed in a way a valid lowering never produces
/// (an out-of-range register or label, a fall off the end), which would itself be a bug
/// worth surfacing in a test.
#[must_use]
pub fn run(program: &Program, args: &[Value]) -> Value {
    let mut regs = alloc_registers(program, args);
    let ops = program.ops();
    let mut pc = program.label_offset(program.entry()).unwrap();

    loop {
        let op = ops[pc];
        match op {
            Op::Const { dst, value } => {
                regs[reg(dst)] = constant(value);
                pc += 1;
            }
            Op::Bin { op, dst, lhs, rhs } => {
                regs[reg(dst)] = binary(op, regs[reg(lhs)], regs[reg(rhs)]);
                pc += 1;
            }
            Op::Un { op, dst, src } => {
                regs[reg(dst)] = unary(op, regs[reg(src)]);
                pc += 1;
            }
            Op::Move { dst, src } => {
                regs[reg(dst)] = regs[reg(src)];
                pc += 1;
            }
            Op::Jump { target } => {
                pc = program.label_offset(target).unwrap();
            }
            Op::JumpUnless { cond, target } => {
                if regs[reg(cond)].boolean() {
                    pc += 1;
                } else {
                    pc = program.label_offset(target).unwrap();
                }
            }
            Op::Return { value } => {
                return value.map_or(Value::Unit, |r| regs[reg(r)]);
            }
        }
    }
}

fn alloc_registers(program: &Program, args: &[Value]) -> Vec<Value> {
    let mut regs = vec![Value::Unit; program.register_count() as usize];
    for (slot, &arg) in program.params().iter().zip(args) {
        regs[reg(*slot)] = arg;
    }
    regs
}

fn reg(r: Reg) -> usize {
    r.0 as usize
}

fn constant(value: Const) -> Value {
    match value {
        Const::Int(v) => Value::Int(v),
        Const::Float(v) => Value::Float(v),
        Const::Bool(v) => Value::Bool(v),
    }
}

fn binary(op: BinOp, lhs: Value, rhs: Value) -> Value {
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div => arithmetic(op, lhs, rhs),
        BinOp::Eq => Value::Bool(lhs == rhs),
        BinOp::Ne => Value::Bool(lhs != rhs),
        BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => ordering(op, lhs, rhs),
        BinOp::And => Value::Bool(lhs.boolean() && rhs.boolean()),
        BinOp::Or => Value::Bool(lhs.boolean() || rhs.boolean()),
    }
}

fn arithmetic(op: BinOp, lhs: Value, rhs: Value) -> Value {
    match (lhs, rhs) {
        (Value::Int(a), Value::Int(b)) => Value::Int(match op {
            BinOp::Add => a.wrapping_add(b),
            BinOp::Sub => a.wrapping_sub(b),
            BinOp::Mul => a.wrapping_mul(b),
            BinOp::Div => a.wrapping_div(b),
            _ => unreachable!(),
        }),
        (Value::Float(a), Value::Float(b)) => Value::Float(match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            _ => unreachable!(),
        }),
        other => panic!("arithmetic on mismatched operands: {other:?}"),
    }
}

fn ordering(op: BinOp, lhs: Value, rhs: Value) -> Value {
    let result = match (lhs, rhs) {
        (Value::Int(a), Value::Int(b)) => compare(op, a, b),
        (Value::Float(a), Value::Float(b)) => compare(op, a, b),
        other => panic!("comparison on mismatched operands: {other:?}"),
    };
    Value::Bool(result)
}

fn compare<T: PartialOrd>(op: BinOp, a: T, b: T) -> bool {
    match op {
        BinOp::Lt => a < b,
        BinOp::Le => a <= b,
        BinOp::Gt => a > b,
        BinOp::Ge => a >= b,
        _ => unreachable!(),
    }
}

fn unary(op: UnOp, operand: Value) -> Value {
    match op {
        UnOp::Neg => match operand {
            Value::Int(v) => Value::Int(v.wrapping_neg()),
            Value::Float(v) => Value::Float(-v),
            other => panic!("negation of a non-numeric value: {other:?}"),
        },
        UnOp::Not => Value::Bool(!operand.boolean()),
    }
}

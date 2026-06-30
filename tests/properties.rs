//! Property tests: codegen must preserve meaning and produce structurally sound output.
//!
//! Random straight-line integer functions are generated as a small data-flow graph,
//! lowered, and run through the reference interpreter. The result is checked against an
//! independent evaluation of the same graph, and the emitted program is checked for the
//! invariants a backend must always uphold — every register in range, every jump
//! resolved.

mod support;

use codegen_lang::{Op, Program, compile};
use ir_lang::{BinOp, Builder, Type};
use proptest::prelude::*;
use support::{Value, run};

/// A node in a straight-line integer data-flow graph: a constant, or a binary operation
/// over two earlier nodes. Restricting operands to earlier nodes keeps every generated
/// function in valid SSA, and restricting operations to wrapping arithmetic keeps every
/// value an integer with a defined result.
#[derive(Clone, Debug)]
enum Node {
    Const(i64),
    Bin(BinOp, usize, usize),
}

/// Resolves raw, unconstrained tuples into a valid graph: node 0 is always a constant,
/// and every operand index is reduced modulo its position so it names an earlier node.
fn resolve(raws: Vec<(bool, i64, usize, usize)>) -> Vec<Node> {
    let mut nodes = Vec::with_capacity(raws.len());
    for (index, (is_bin, value, a, b)) in raws.into_iter().enumerate() {
        if !is_bin || index == 0 {
            nodes.push(Node::Const(value));
        } else {
            let op = [BinOp::Add, BinOp::Sub, BinOp::Mul][(value.unsigned_abs() % 3) as usize];
            nodes.push(Node::Bin(op, a % index, b % index));
        }
    }
    nodes
}

fn graph() -> impl Strategy<Value = Vec<Node>> {
    proptest::collection::vec(
        (any::<bool>(), any::<i64>(), any::<usize>(), any::<usize>()),
        1..24,
    )
    .prop_map(resolve)
}

/// Evaluates the graph directly, the oracle the compiled program is checked against.
fn evaluate(nodes: &[Node]) -> i64 {
    let mut values: Vec<i64> = Vec::with_capacity(nodes.len());
    for node in nodes {
        let value = match *node {
            Node::Const(c) => c,
            Node::Bin(op, a, b) => apply(op, values[a], values[b]),
        };
        values.push(value);
    }
    *values.last().expect("a graph has at least one node")
}

fn apply(op: BinOp, a: i64, b: i64) -> i64 {
    match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        _ => unreachable!("the generator only emits add, sub, and mul"),
    }
}

/// Builds the IR function the graph describes.
fn build(nodes: &[Node]) -> ir_lang::Function {
    let mut b = Builder::new("p", &[], Type::Int);
    let mut values = Vec::with_capacity(nodes.len());
    for node in nodes {
        let value = match *node {
            Node::Const(c) => b.iconst(c),
            Node::Bin(op, a, c) => b.bin(op, values[a], values[c]),
        };
        values.push(value);
    }
    b.ret(Some(*values.last().expect("a graph has at least one node")));
    b.finish()
}

/// Confirms every op references registers in range and every jump resolves to a real op.
fn assert_structurally_sound(program: &Program) {
    let registers = program.register_count();
    let op_count = program.len();
    let check_reg = |r: codegen_lang::Reg| assert!(r.0 < registers, "register {r} out of range");
    let check_label = |program: &Program, target| {
        let offset = program
            .label_offset(target)
            .expect("every jump target must resolve");
        assert!(offset < op_count, "label {target} points past the ops");
    };

    for &op in program.ops() {
        match op {
            Op::Const { dst, .. } => check_reg(dst),
            Op::Bin { dst, lhs, rhs, .. } => {
                check_reg(dst);
                check_reg(lhs);
                check_reg(rhs);
            }
            Op::Un { dst, src, .. } => {
                check_reg(dst);
                check_reg(src);
            }
            Op::Move { dst, src } => {
                check_reg(dst);
                check_reg(src);
            }
            Op::Jump { target } => check_label(program, target),
            Op::JumpUnless { cond, target } => {
                check_reg(cond);
                check_label(program, target);
            }
            Op::Return { value } => {
                if let Some(r) = value {
                    check_reg(r);
                }
            }
        }
    }
}

proptest! {
    #[test]
    fn compiled_program_matches_a_direct_evaluation(nodes in graph()) {
        let program = compile(&build(&nodes)).expect("a straight-line graph is valid");
        prop_assert_eq!(run(&program, &[]), Value::Int(evaluate(&nodes)));
    }

    #[test]
    fn compiled_program_is_structurally_sound(nodes in graph()) {
        let program = compile(&build(&nodes)).expect("a straight-line graph is valid");
        assert_structurally_sound(&program);
    }

    #[test]
    fn register_count_equals_the_number_of_values(nodes in graph()) {
        let program = compile(&build(&nodes)).expect("a straight-line graph is valid");
        // Every node defines exactly one value, hence one register.
        prop_assert_eq!(program.register_count() as usize, nodes.len());
    }
}

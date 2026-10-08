//! Block-argument moves on loop back edges: the parallel-copy semantics of SSA edges.
//!
//! The arguments of a control-flow edge are one parallel assignment — all read, then all
//! written. On a loop back edge that passes the header's own parameters, sources and
//! destinations overlap, so the moves must be ordered (and a cycle broken through a
//! scratch register) or a parameter is overwritten before it is read. Until 1.0.1 the
//! backend emitted them in argument order and miscompiled exactly these shapes.
//!
//! The named tests pin the shapes that broke (swap, rotation, shift, duplication, and a
//! two-armed back edge). The properties generate whole CFGs with loops whose back edges
//! carry arbitrary permutations, rotations, duplications, and mixes of parameters and
//! fresh values, and check every lowered program three ways: its result against a direct
//! evaluation of the IR (`support::ir`), its structure against the program invariants,
//! and its move count against the minimum.

mod support;

use codegen_lang::{Op, Program, Reg, compile};
use ir_lang::{BinOp, Builder, Function, Terminator, Type, Value as IrValue};
use proptest::prelude::*;
use support::{Value, ir, run_bounded};

/// Blocks the IR evaluator may enter before a test is declared non-terminating.
const IR_FUEL: usize = 100_000;
/// Ops the bytecode interpreter may execute before a test is declared non-terminating.
const OP_FUEL: usize = 10_000_000;

/// Compiles `func`, runs it and the IR evaluator on `args`, asserts they agree and that
/// the program is sound, and returns the result.
fn check(func: &Function, args: &[Value]) -> Value {
    let program = compile(func).expect("the test function is well-formed");
    if let Err(defect) = program.__check_invariants() {
        panic!("lowered program violates an invariant: {defect}\n{program}");
    }
    let expected = ir::eval(func, args, IR_FUEL);
    let actual = run_bounded(&program, args, OP_FUEL);
    assert_eq!(
        actual, expected,
        "bytecode disagrees with the IR\n{program}"
    );
    actual
}

/// Builds a counted loop over a header `H(c, p0, p1, …)` whose back edge passes
/// `c - 1` and the data arguments `back(params, body_value)`, where `body_value` is a
/// fresh value computed in the body (`p0 + 10`). Returns `fold(params)` on exit, a
/// position-sensitive combination so a misplaced value changes the result.
fn counted_loop(width: usize, back: impl Fn(&[IrValue], IrValue) -> Vec<IrValue>) -> Function {
    // The function's parameters are the counter and the initial data, passed straight
    // into the header.
    let types = vec![Type::Int; width + 1];
    let mut b = Builder::new("loop", &types, Type::Int);
    let entry_params = b.block_params(b.entry()).to_vec();
    let header = b.create_block(&types);
    let body = b.create_block(&[]);
    let exit = b.create_block(&[]);
    b.jump(header, &entry_params);

    b.switch_to(header);
    let hp = b.block_params(header).to_vec();
    let (c, data) = (hp[0], hp[1..].to_vec());
    let zero = b.iconst(0);
    let more = b.bin(BinOp::Gt, c, zero);
    b.branch(more, body, &[], exit, &[]);

    b.switch_to(body);
    let one = b.iconst(1);
    let next = b.bin(BinOp::Sub, c, one);
    let ten = b.iconst(10);
    let fresh = b.bin(BinOp::Add, data[0], ten);
    let mut args = vec![next];
    args.extend(back(&data, fresh));
    b.jump(header, &args);

    b.switch_to(exit);
    let result = fold(&mut b, &data);
    b.ret(Some(result));
    b.finish()
}

/// `((seed * 1000 + p0) * 1000 + p1) …` — each parameter lands in its own decimal digits
/// for small inputs, so a result reads back as the final parameter tuple.
fn fold(b: &mut Builder, values: &[IrValue]) -> IrValue {
    let base = b.iconst(1000);
    let mut acc = b.iconst(9);
    for &v in values {
        let scaled = b.bin(BinOp::Mul, acc, base);
        acc = b.bin(BinOp::Add, scaled, v);
    }
    acc
}

fn ints(values: &[i64]) -> Vec<Value> {
    values.iter().map(|&v| Value::Int(v)).collect()
}

/// The moves a [`counted_loop`] spends on its back edge: all of its moves except the
/// `width + 1` that carry the function's parameters into the header on entry.
fn back_edge_moves(program: &Program, width: usize) -> usize {
    moves(program) - (width + 1)
}

fn moves(program: &Program) -> usize {
    program
        .ops()
        .iter()
        .filter(|op| matches!(op, Op::Move { .. }))
        .count()
}

#[test]
fn test_back_edge_swap_exchanges_the_parameters() {
    // H(c, a, b): … jump H(c - 1, b, a). The 1.0.0 lowering emitted `a ← b; b ← a`.
    let func = counted_loop(2, |p, _| vec![p[1], p[0]]);
    // Zero, one, two, three trips: the pair alternates.
    assert_eq!(check(&func, &ints(&[0, 1, 2])), Value::Int(9_001_002));
    assert_eq!(check(&func, &ints(&[1, 1, 2])), Value::Int(9_002_001));
    assert_eq!(check(&func, &ints(&[2, 1, 2])), Value::Int(9_001_002));
    assert_eq!(check(&func, &ints(&[3, 1, 2])), Value::Int(9_002_001));

    // The swap costs three moves through the scratch register, plus the counter's move;
    // the scratch register is the one register past the function's values.
    let program = compile(&func).expect("well-formed");
    assert_eq!(back_edge_moves(&program, 2), 4);
    assert_eq!(program.register_count() as usize, func.value_count() + 1);
    let scratch = Reg(func.value_count() as u32);
    assert!(
        program
            .ops()
            .iter()
            .any(|op| matches!(op, Op::Move { dst, .. } if *dst == scratch))
    );
}

#[test]
fn test_back_edge_three_cycle_rotation() {
    // H(c, a, b, d): … jump H(c - 1, b, d, a).
    let func = counted_loop(3, |p, _| vec![p[1], p[2], p[0]]);
    assert_eq!(
        check(&func, &ints(&[1, 1, 2, 3])),
        Value::Int(9_002_003_001)
    );
    assert_eq!(
        check(&func, &ints(&[2, 1, 2, 3])),
        Value::Int(9_003_001_002)
    );
    assert_eq!(
        check(&func, &ints(&[3, 1, 2, 3])),
        Value::Int(9_001_002_003)
    );
    // A 3-cycle costs 3 + 1 moves, plus the counter's move.
    assert_eq!(back_edge_moves(&compile(&func).expect("well-formed"), 3), 5);
}

#[test]
fn test_back_edge_duplication() {
    // H(c, a, b): … jump H(c - 1, a, a). `a ← a` is skipped; `b ← a` needs no scratch.
    let func = counted_loop(2, |p, _| vec![p[0], p[0]]);
    assert_eq!(check(&func, &ints(&[1, 1, 2])), Value::Int(9_001_001));
    let program = compile(&func).expect("well-formed");
    assert_eq!(back_edge_moves(&program, 2), 2);
    assert_eq!(program.register_count() as usize, func.value_count());
}

#[test]
fn test_back_edge_duplicating_the_other_parameter() {
    // H(c, a, b): … jump H(c - 1, b, b): `a ← b` must not be clobbered by anything.
    let func = counted_loop(2, |p, _| vec![p[1], p[1]]);
    assert_eq!(check(&func, &ints(&[1, 1, 2])), Value::Int(9_002_002));
}

#[test]
fn test_back_edge_shift_reads_before_it_writes() {
    // H(c, a, b): a2 = a + 10; jump H(c - 1, a2, a). In argument order, `a ← a2` ran
    // first and `b ← a` then copied the *new* a.
    let func = counted_loop(2, |p, fresh| vec![fresh, p[0]]);
    assert_eq!(check(&func, &ints(&[1, 1, 2])), Value::Int(9_011_001));
    assert_eq!(check(&func, &ints(&[2, 1, 2])), Value::Int(9_021_011));
    // A chain, not a cycle: two moves plus the counter's, no scratch register.
    let program = compile(&func).expect("well-formed");
    assert_eq!(back_edge_moves(&program, 2), 3);
    assert_eq!(program.register_count() as usize, func.value_count());
}

#[test]
fn test_back_edge_reversal_of_four() {
    // Two 2-cycles: (p0 p3)(p1 p2). Each costs three moves.
    let func = counted_loop(4, |p, _| vec![p[3], p[2], p[1], p[0]]);
    assert_eq!(
        check(&func, &ints(&[1, 1, 2, 3, 4])),
        Value::Int(9_004_003_002_001)
    );
    assert_eq!(
        back_edge_moves(&compile(&func).expect("well-formed"), 4),
        1 + 6
    );
}

#[test]
fn test_wide_back_edge_mixing_cycles_chains_and_duplicates() {
    // 40 data parameters (past the 16-copy stack path, so the table scheduler runs): slot
    // 0 takes a fresh value, slot 1 duplicates p0, slots 2..20 rotate among themselves
    // (one 18-cycle), and slots 20..40 reverse (ten 2-cycles).
    let width = 40;
    let func = counted_loop(width, |p, fresh| {
        (0..width)
            .map(|j| match j {
                0 => fresh,
                1 => p[0],
                2..20 => p[2 + (j - 2 + 5) % 18],
                _ => p[59 - j],
            })
            .collect()
    });
    for trips in 0..5 {
        let mut args = vec![trips];
        args.extend(1..=width as i64);
        check(&func, &ints(&args));
    }
    let program = compile(&func).expect("well-formed");
    assert_eq!(moves(&program), minimal_moves(&func));
    // 1 counter + 2 (fresh, duplicate) + 18 + 1 + 10 * 3, plus the entry edge.
    assert_eq!(back_edge_moves(&program, width), 1 + 2 + 19 + 30);
}

#[test]
fn test_two_armed_back_edge_permutes_each_arm_independently() {
    // H(c, a, b, d): body branches on (c is odd) straight back to H, with a swap on one
    // arm and a rotation on the other. Both arms target the same block, so each arm's
    // moves are a separate parallel copy and the arm not taken must not run.
    let mut b = Builder::new(
        "arms",
        &[Type::Int, Type::Int, Type::Int, Type::Int],
        Type::Int,
    );
    let entry_params = b.block_params(b.entry()).to_vec();
    let header = b.create_block(&[Type::Int, Type::Int, Type::Int, Type::Int]);
    let body = b.create_block(&[]);
    let exit = b.create_block(&[]);
    b.jump(header, &entry_params);

    b.switch_to(header);
    let hp = b.block_params(header).to_vec();
    let zero = b.iconst(0);
    let more = b.bin(BinOp::Gt, hp[0], zero);
    b.branch(more, body, &[], exit, &[]);

    b.switch_to(body);
    let one = b.iconst(1);
    let next = b.bin(BinOp::Sub, hp[0], one);
    let two = b.iconst(2);
    let half = b.bin(BinOp::Div, hp[0], two);
    let twice = b.bin(BinOp::Mul, half, two);
    let odd = b.bin(BinOp::Ne, twice, hp[0]);
    b.branch(
        odd,
        header,
        &[next, hp[2], hp[1], hp[3]], // swap a, b
        header,
        &[next, hp[2], hp[3], hp[1]], // rotate a, b, d
    );

    b.switch_to(exit);
    let result = fold(&mut b, &hp[1..]);
    b.ret(Some(result));
    let func = b.finish();

    for trips in 0..8 {
        check(&func, &ints(&[trips, 1, 2, 3]));
    }
    // c = 3 (odd: swap) -> (2,1,3); c = 2 (even: rotate) -> (1,3,2); c = 1 (swap) -> (3,1,2).
    assert_eq!(
        check(&func, &ints(&[3, 1, 2, 3])),
        Value::Int(9_003_001_002)
    );
}

#[test]
fn test_forward_edges_keep_argument_order() {
    // A conflict-free edge is emitted in argument order, as 1.0.0 did: lowering of
    // programs without overlapping copies is unchanged.
    let mut b = Builder::new("f", &[Type::Int, Type::Int], Type::Int);
    let p = b.block_params(b.entry()).to_vec();
    let next = b.create_block(&[Type::Int, Type::Int]);
    b.jump(next, &[p[1], p[0]]);
    b.switch_to(next);
    let q = b.block_params(next).to_vec();
    let r = b.bin(BinOp::Sub, q[0], q[1]);
    b.ret(Some(r));
    let func = b.finish();

    let program = compile(&func).expect("well-formed");
    let reg = |v: IrValue| Reg(v.index() as u32);
    assert_eq!(
        &program.ops()[..2],
        [
            Op::Move {
                dst: reg(q[0]),
                src: reg(p[1])
            },
            Op::Move {
                dst: reg(q[1]),
                src: reg(p[0])
            },
        ]
    );
    assert_eq!(check(&func, &ints(&[3, 10])), Value::Int(7));
}

// ---------------------------------------------------------------------------------------
// Generated CFGs.
// ---------------------------------------------------------------------------------------

/// How an edge fills the data parameters of its target.
#[derive(Clone, Debug)]
enum Pattern {
    /// Each target slot `j` takes source parameter `j` (wrapping): self-moves on a back
    /// edge.
    Identity,
    /// Slot `j` takes source parameter `j + k`: one or more cycles on a back edge.
    Rotate(usize),
    /// Slot `j` takes source parameter `w - 1 - j`: 2-cycles on a back edge.
    Reverse,
    /// Slot `j` takes source parameter `perm[j]`, a permutation drawn from sort keys.
    Permute(Vec<u16>),
    /// Every slot takes the same source parameter: duplication.
    Duplicate(usize),
    /// Slot `j` takes an arbitrary value from the edge's whole pool: parameters, the
    /// counter, the target's previous parameters, fresh body values, function inputs.
    Mixed(Vec<usize>),
}

/// One counted loop of the generated function.
#[derive(Clone, Debug)]
struct Stage {
    /// Data parameters of the header (besides the counter).
    width: usize,
    /// Iterations the loop runs: the counter's initial value.
    trips: i64,
    /// How the edge into this header fills its data parameters.
    entry: Pattern,
    /// Parameters of the body block, filled from the header on the loop-entering arm.
    body_params: Vec<usize>,
    /// Fresh body values: `(op selector, lhs pick, rhs pick)` over the body's pool.
    body_values: Vec<(u8, usize, usize)>,
    /// The back edge: one jump, or a branch whose two arms both return to the header.
    back: Back,
}

#[derive(Clone, Debug)]
enum Back {
    Jump(Pattern),
    Branch {
        cond: (usize, usize),
        then_args: Pattern,
        else_args: Pattern,
    },
}

#[derive(Clone, Debug)]
struct Spec {
    inputs: Vec<i64>,
    stages: Vec<Stage>,
    /// How the final exit block's parameters are filled from the last header.
    exit: Pattern,
    exit_width: usize,
}

fn pattern() -> impl Strategy<Value = Pattern> {
    prop_oneof![
        Just(Pattern::Identity),
        (1_usize..6).prop_map(Pattern::Rotate),
        Just(Pattern::Reverse),
        proptest::collection::vec(any::<u16>(), 6).prop_map(Pattern::Permute),
        (0_usize..6).prop_map(Pattern::Duplicate),
        proptest::collection::vec(any::<usize>(), 6).prop_map(Pattern::Mixed),
    ]
}

fn back() -> impl Strategy<Value = Back> {
    prop_oneof![
        pattern().prop_map(Back::Jump),
        ((any::<usize>(), any::<usize>()), pattern(), pattern()).prop_map(
            |(cond, then_args, else_args)| Back::Branch {
                cond,
                then_args,
                else_args
            }
        ),
    ]
}

fn stage() -> impl Strategy<Value = Stage> {
    (
        1_usize..6,
        0_i64..5,
        pattern(),
        proptest::collection::vec(any::<usize>(), 0..3),
        proptest::collection::vec((any::<u8>(), any::<usize>(), any::<usize>()), 0..4),
        back(),
    )
        .prop_map(
            |(width, trips, entry, body_params, body_values, back)| Stage {
                width,
                trips,
                entry,
                body_params,
                body_values,
                back,
            },
        )
}

fn spec() -> impl Strategy<Value = Spec> {
    (
        proptest::collection::vec(-1000_i64..1000, 1..5),
        proptest::collection::vec(stage(), 1..4),
        pattern(),
        1_usize..5,
    )
        .prop_map(|(inputs, stages, exit, exit_width)| Spec {
            inputs,
            stages,
            exit,
            exit_width,
        })
}

/// Resolves a pattern to `width` arguments. `params` are the "own" parameters the
/// structured patterns permute; `pool` is everything the edge may legally pass.
fn resolve(pattern: &Pattern, width: usize, params: &[IrValue], pool: &[IrValue]) -> Vec<IrValue> {
    let w = params.len();
    (0..width)
        .map(|j| match pattern {
            Pattern::Identity => params[j % w],
            Pattern::Rotate(k) => params[(j + k) % w],
            Pattern::Reverse => params[(w - 1) - (j % w)],
            Pattern::Permute(keys) => {
                let mut order: Vec<usize> = (0..w).collect();
                order.sort_by_key(|&i| (keys[i % keys.len()], i));
                params[order[j % w]]
            }
            Pattern::Duplicate(i) => params[i % w],
            Pattern::Mixed(picks) => pool[picks[j % picks.len()] % pool.len()],
        })
        .collect()
}

/// Builds the function a spec describes:
///
/// ```text
/// entry(x…):            jump H0(t0, entry-pattern over x…)
/// Hi(c, p…):            more = c > 0; next_t = t(i+1)
///                       branch more, Bi(body params from c, p…),
///                              H(i+1)(next_t, …) | X(…)
/// Bi(q…):               c' = c - 1; fresh values over (c, p…, q…, x…)
///                       jump Hi(c', back-pattern) | branch k, Hi(c', A), Hi(c', B)
/// X(r…):                return fold(r…)
/// ```
///
/// Every loop terminates (the counter strictly falls on every back edge and the header
/// exits at zero), and every argument is drawn from values that dominate its edge, so
/// the function always validates.
fn build(spec: &Spec) -> Function {
    let input_types = vec![Type::Int; spec.inputs.len()];
    let mut b = Builder::new("cfg", &input_types, Type::Int);
    let inputs = b.block_params(b.entry()).to_vec();

    let headers: Vec<_> = spec
        .stages
        .iter()
        .map(|s| b.create_block(&vec![Type::Int; s.width + 1]))
        .collect();
    let bodies: Vec<_> = spec
        .stages
        .iter()
        .map(|s| b.create_block(&vec![Type::Int; s.body_params.len()]))
        .collect();
    let exit = b.create_block(&vec![Type::Int; spec.exit_width]);

    // entry -> H0
    let first = &spec.stages[0];
    let t0 = b.iconst(first.trips);
    let mut args = vec![t0];
    args.extend(resolve(&first.entry, first.width, &inputs, &inputs));
    b.jump(headers[0], &args);

    for (i, stage) in spec.stages.iter().enumerate() {
        let header = headers[i];
        let body = bodies[i];

        // Header: test the counter, then enter the body or leave for the next stage.
        b.switch_to(header);
        let hp = b.block_params(header).to_vec();
        let (c, data) = (hp[0], hp[1..].to_vec());
        let zero = b.iconst(0);
        let more = b.bin(BinOp::Gt, c, zero);
        let mut header_pool = vec![c];
        header_pool.extend(&data);
        header_pool.extend(&inputs);

        let body_args: Vec<IrValue> = stage
            .body_params
            .iter()
            .map(|&pick| header_pool[pick % header_pool.len()])
            .collect();
        let (exit_block, exit_args) = match spec.stages.get(i + 1) {
            Some(next) => {
                let t = b.iconst(next.trips);
                let mut args = vec![t];
                args.extend(resolve(&next.entry, next.width, &data, &header_pool));
                (headers[i + 1], args)
            }
            None => (
                exit,
                resolve(&spec.exit, spec.exit_width, &data, &header_pool),
            ),
        };
        b.branch(more, body, &body_args, exit_block, &exit_args);

        // Body: count down, compute fresh values, and go back to the header.
        b.switch_to(body);
        let one = b.iconst(1);
        let next = b.bin(BinOp::Sub, c, one);
        let mut pool = header_pool.clone();
        pool.extend(b.block_params(body).to_vec());
        pool.push(next);
        for &(op, lhs, rhs) in &stage.body_values {
            let op = [BinOp::Add, BinOp::Sub, BinOp::Mul][usize::from(op) % 3];
            let value = b.bin(op, pool[lhs % pool.len()], pool[rhs % pool.len()]);
            pool.push(value);
        }

        let back_args = |pattern: &Pattern| {
            let mut args = vec![next];
            args.extend(resolve(pattern, stage.width, &data, &pool));
            args
        };
        match &stage.back {
            Back::Jump(pattern) => {
                let args = back_args(pattern);
                b.jump(header, &args);
            }
            Back::Branch {
                cond: (x, y),
                then_args,
                else_args,
            } => {
                let (then_args, else_args) = (back_args(then_args), back_args(else_args));
                let cond = b.bin(BinOp::Lt, pool[x % pool.len()], pool[y % pool.len()]);
                b.branch(cond, header, &then_args, header, &else_args);
            }
        }
    }

    b.switch_to(exit);
    let rp = b.block_params(exit).to_vec();
    let result = fold(&mut b, &rp);
    b.ret(Some(result));
    b.finish()
}

/// The fewest moves a correct lowering can emit for `func`: per edge, one per argument
/// whose register differs from its parameter's, plus one per cycle among those copies.
/// Computed here independently of the backend, straight from the IR.
fn minimal_moves(func: &Function) -> usize {
    let mut total = 0;
    let mut edge = |target: ir_lang::Block, args: &[IrValue]| {
        let copies: Vec<(usize, usize)> = func
            .block_params(target)
            .iter()
            .zip(args)
            .map(|(p, a)| (p.index(), a.index()))
            .filter(|(d, s)| d != s)
            .collect();
        let source_of = |r: usize| copies.iter().find(|(d, _)| *d == r).map(|&(_, s)| s);
        let mut cycles = 0;
        for &(start, _) in &copies {
            // Walk back along sources; count each cycle once, at its smallest register.
            let (mut at, mut smallest, mut steps) = (start, start, 0);
            while let Some(s) = source_of(at) {
                at = s;
                smallest = smallest.min(at);
                steps += 1;
                if at == start || steps > copies.len() {
                    break;
                }
            }
            if at == start && smallest == start {
                cycles += 1;
            }
        }
        total += copies.len() + cycles;
    };
    for block in func.blocks() {
        match func.terminator(block).expect("terminated") {
            Terminator::Return(_) => {}
            Terminator::Jump(target, args) => edge(*target, args),
            Terminator::Branch {
                then_block,
                then_args,
                else_block,
                else_args,
                ..
            } => {
                edge(*then_block, then_args);
                edge(*else_block, else_args);
            }
        }
    }
    total
}

/// Whether the scratch register (the one past the function's values) appears anywhere.
fn uses_scratch(program: &Program, func: &Function) -> bool {
    let scratch = Reg(func.value_count() as u32);
    program.ops().iter().any(|op| match *op {
        Op::Move { dst, src } => dst == scratch || src == scratch,
        _ => false,
    })
}

#[test]
fn test_generator_covers_the_shapes_that_matter() {
    // The properties below are only as strong as the CFGs they see. Sample the generator
    // deterministically and require that it really produces back-edge cycles (scratch
    // register used), duplicated arguments, two-armed back edges, multi-trip loops, and
    // programs that are correct only if their moves are ordered.
    use proptest::strategy::ValueTree;
    use proptest::test_runner::TestRunner;

    let mut runner = TestRunner::deterministic();
    let strategy = spec();
    let (mut cycles, mut duplicated, mut two_armed, mut looped) = (0, 0, 0, 0);
    let samples = 512;
    for _ in 0..samples {
        let spec = strategy
            .new_tree(&mut runner)
            .expect("strategy builds")
            .current();
        let func = build(&spec);
        let program = compile(&func).expect("validated");
        cycles += usize::from(uses_scratch(&program, &func));
        for stage in &spec.stages {
            duplicated += usize::from(matches!(stage.back, Back::Jump(Pattern::Duplicate(_))));
            two_armed += usize::from(matches!(stage.back, Back::Branch { .. }));
            looped += usize::from(stage.trips >= 2);
        }
    }
    assert!(
        cycles * 5 >= samples,
        "only {cycles}/{samples} with a cycle"
    );
    assert!(duplicated > 0 && two_armed > 0 && looped > 0);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn generated_cfg_matches_direct_ir_evaluation(spec in spec()) {
        let func = build(&spec);
        prop_assert!(func.validate().is_ok(), "generator built invalid IR: {:?}", func.validate());
        let program = compile(&func).expect("validated");
        let args = ints(&spec.inputs);
        let expected = ir::eval(&func, &args, IR_FUEL);
        let actual = run_bounded(&program, &args, OP_FUEL);
        prop_assert_eq!(actual, expected, "\n{}", program);
    }

    #[test]
    fn generated_cfg_lowers_to_a_sound_program(spec in spec()) {
        let func = build(&spec);
        let program = compile(&func).expect("validated");
        prop_assert!(program.__check_invariants().is_ok(), "{:?}\n{}", program.__check_invariants(), program);
        // No self-moves: a copy onto itself is always skipped.
        let self_move = program.ops().iter().any(|op| matches!(op, Op::Move { dst, src } if dst == src));
        prop_assert!(!self_move, "a self-move was emitted
{}", program);
    }

    #[test]
    fn generated_cfg_move_count_is_minimal(spec in spec()) {
        let func = build(&spec);
        let program = compile(&func).expect("validated");
        prop_assert_eq!(moves(&program), minimal_moves(&func), "\n{}", program);
    }

    #[test]
    fn register_count_is_exact(spec in spec()) {
        // The value registers, plus the scratch register exactly when some edge used it.
        let func = build(&spec);
        let program = compile(&func).expect("validated");
        let expected = func.value_count() + usize::from(uses_scratch(&program, &func));
        prop_assert_eq!(program.register_count() as usize, expected);
    }
}

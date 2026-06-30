//! Codegen throughput benchmarks.
//!
//! Each benchmark builds an IR function once, then measures only the lowering — the work
//! [`codegen_lang::compile`] does per call — so the numbers track the backend, not the
//! builder. Three shapes cover the cost drivers: a flat run of instructions, a wide fan
//! of two-way branches, and a loop with block-parameter edges.

use codegen_lang::compile;
use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use ir_lang::{BinOp, Builder, Function, Type};

/// A straight-line function of `n` chained additions: the simplest lowering path, one op
/// per value with no control flow.
fn straight_line(n: u32) -> Function {
    let mut b = Builder::new("straight", &[Type::Int], Type::Int);
    let mut acc = b.block_params(b.entry())[0];
    for i in 0..n {
        let k = b.iconst(i64::from(i));
        acc = b.bin(BinOp::Add, acc, k);
    }
    b.ret(Some(acc));
    b.finish()
}

/// A chain of `n` diamonds: each picks the larger of two running values and feeds the
/// next, exercising branch lowering and edge moves at scale.
fn branch_chain(n: u32) -> Function {
    let mut b = Builder::new("branches", &[Type::Int, Type::Int], Type::Int);
    let mut a = b.block_params(b.entry())[0];
    let mut c = b.block_params(b.entry())[1];

    for _ in 0..n {
        let join = b.create_block(&[Type::Int]);
        let then_blk = b.create_block(&[]);
        let else_blk = b.create_block(&[]);
        let cond = b.bin(BinOp::Lt, a, c);
        b.branch(cond, then_blk, &[], else_blk, &[]);
        b.switch_to(then_blk);
        b.jump(join, &[c]);
        b.switch_to(else_blk);
        b.jump(join, &[a]);
        b.switch_to(join);
        a = b.block_params(join)[0];
        c = b.iconst(1);
    }
    b.ret(Some(a));
    b.finish()
}

/// A countdown loop summing `n .. 0`, a header carrying two block parameters with a
/// back-edge — the shape that stresses the parameter-move path.
fn loop_sum() -> Function {
    let mut b = Builder::new("loop_sum", &[Type::Int], Type::Int);
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

fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");

    for &n in &[16u32, 256, 4096] {
        let func = straight_line(n);
        group.throughput(Throughput::Elements(u64::from(n)));
        group.bench_with_input(BenchmarkId::new("straight_line", n), &func, |bencher, f| {
            bencher.iter(|| compile(std::hint::black_box(f)));
        });
    }

    for &n in &[8u32, 64, 512] {
        let func = branch_chain(n);
        group.throughput(Throughput::Elements(u64::from(n)));
        group.bench_with_input(BenchmarkId::new("branch_chain", n), &func, |bencher, f| {
            bencher.iter(|| compile(std::hint::black_box(f)));
        });
    }

    let func = loop_sum();
    group.bench_function("loop_sum", |bencher| {
        bencher.iter(|| compile(std::hint::black_box(&func)));
    });

    group.finish();
}

criterion_group!(benches, bench_compile);
criterion_main!(benches);

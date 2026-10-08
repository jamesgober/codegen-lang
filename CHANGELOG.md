<h1 align="center">
    <img width="90px" height="auto" src="https://raw.githubusercontent.com/jamesgober/jamesgober/main/media/icons/hexagon-3.svg" alt="Triple Hexagon">
    <br><b>CHANGELOG</b>
</h1>
<p>
  All notable changes to <code>codegen-lang</code> will be documented in this file. The format is based on <a href="https://keepachangelog.com/en/1.1.0/">Keep a Changelog</a>,
  and this project adheres to <a href="https://semver.org/spec/v2.0.0.html/">Semantic Versioning</a>.
</p>

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [1.0.1] - 2026-10-08

A correctness patch. Block-argument moves on loop back edges were miscompiled when an edge passed its target block's own parameters (`H(a, b): … jump H(b, a)`); they are now sequentialized as the parallel copy SSA defines. No public API change. `aot-lang`, which depends on `codegen-lang = "1"`, picks the fix up on its next dependency resolution with no change of its own.

### Fixed

- **Miscompile of permuting back edges (ISSUES H01).** The arguments of a control-flow edge were copied in argument order on the claim that an edge's source and destination registers are disjoint in SSA. That is false for a loop back edge that passes the header's own parameters: a swap `jump H(b, a)` lowered to `a ← b; b ← a` (both ended up `b`), and a shift `jump H(a2, a)` copied the *new* `a` into `b`. ir-lang's validator accepts these edges, so valid IR produced a wrong program. Each edge's copies are now scheduled as a parallel move: a copy runs only once no pending copy still reads its destination, and each remaining cycle is broken through one scratch register. Self-copies are still skipped, and the move count is minimal — one per copy, plus one per cycle (a cycle of length `k` costs `k + 1` moves). Both arms of a two-way branch and plain jumps go through the same scheduler. Edges whose copies do not conflict lower exactly as before, in argument order. An edge of up to 16 copies is scheduled on the stack with no heap use; larger edges use per-register tables allocated once per function, in linear time. Cost: unchanged for functions without conflicting copies; a single 131072-copy cycle compiles about 1.7× slower than 1.0.0's (incorrect) single pass. See the release notes for the numbers.
- The false "disjoint, so no parallel-move scheduling is needed" claim is removed from the README and the lowering docs.

### Changed

- **`Program::register_count` includes the scratch register when it is used.** A function with at least one cyclic edge gets one extra register, numbered just past its value registers; every other function's count is unchanged (still its value count). The count stays exact: one past the highest register any op names.
- Every unchecked `as u32` cast in the lowering is replaced by a checked conversion that cannot panic or wrap. Register numbers and block labels come from ir-lang's `u32` handles and always fit; register counts, internal labels, and op offsets exceed `u32` only for a program with more than `u32::MAX` registers, labels, or ops, which the bytecode format cannot describe. There the conversion saturates instead of silently aliasing a register or label. The bound is documented on the conversion (`src/moves.rs`, `narrow`).
- The disassembly `Display` for `Program` no longer scans every label for every op: it sorts the labels once and walks them alongside the ops, `O(ops + labels·log labels)` instead of `O(ops × labels)`. The output is byte-for-byte the same, for lowered and deserialized programs alike (tested against the old loop).

### Added

- `Program::__check_invariants` — a `#[doc(hidden)]` structural check (ops non-empty, entry label at op 0, every register and parameter in range, parameters distinct, every label and jump target inside the op stream, last op a jump or return). It is **not** part of the stable API and is exempt from SemVer; it lets tests check lowered and deserialized programs without a public API change in a patch. A public, documented `Program::validate` is planned for a minor release.
- `tests/parallel_moves.rs` — named regressions for the swap, 3-cycle rotation, shift, duplication (`jump H(a, a)`), four-way reversal, a 40-parameter mixed back edge (exercising the large-edge scheduler), and a two-armed back edge that permutes differently on each arm; and property tests over generated CFGs with loops whose back edges carry arbitrary permutations, rotations, duplications, and mixes of parameters, fresh values, and function inputs. Each generated program is run and compared with a direct evaluation of the IR (a new IR interpreter in `tests/support/ir.rs`), checked against the program invariants, checked for a minimal move count, and checked for an exact register count. A coverage test confirms the generator really produces cycles (about two thirds of samples).
- Unit and property tests for the move scheduler against a parallel-assignment reference, including a 100k-register rotation.
- `rotating_loop` benchmark: a back edge that rotates `n` parameters (one `n`-cycle), at 16, 1024, and 131072.

---

## [1.0.0] - 2026-06-30

API freeze. The public surface built in 0.2.0 is ratified as the `1.0` contract: it follows Semantic Versioning and carries no breaking changes before `2.0`. There is no breaking change from `0.2.0` — a `0.2.0` program compiles and behaves identically. The freeze adds only what hardens the release: runnable examples, serialization round-trip coverage, and the stability documentation.

### Added

- `examples/disassemble.rs` — lowers `double`, `abs`, and a loop and prints each one's disassembly.
- `examples/inspect.rs` — walks a compiled program's op stream and shows the error path on invalid IR.
- `serde` round-trip tests for `Program` and `CodegenError` (`tests/serde.rs`).

### Changed

- Marked the public API stable and frozen as of `1.0.0`; recorded the SemVer promise in [`docs/API.md`](docs/API.md#semver-promise) and the crate root.

---

## [0.2.0] - 2026-06-30

The core milestone: the backend abstraction and the bytecode reference backend land. This is the first release with domain logic — it lowers an `ir-lang` function in SSA form to a flat, register-based bytecode program. The public surface is documented in [`docs/API.md`](docs/API.md) and remains pre-1.0 (subject to change until the `1.0.0` freeze).

### Added

- `Backend` trait — the code-generation abstraction: lowers an `ir_lang::Function` to a backend-defined `Output`, so LLVM or Cranelift targets can be added behind the same interface.
- `Bytecode` — the reference backend: validates a function, then lowers each block to a linear op stream.
- `compile` — shortcut for lowering with the default `Bytecode` backend.
- `Program` — the lowered function: name, parameter registers, register count, op stream, label resolution, and a disassembly `Display`.
- `Op` — the closed bytecode instruction set (`Const`, `Bin`, `Un`, `Move`, `Jump`, `JumpUnless`, `Return`).
- `Reg`, `Label`, `Const` — registers, jump targets, and constant operands, each with a `Display`.
- `CodegenError` — the failure reason; `InvalidIr` wraps the `ir-lang` validation error, with `Display`, `Error`, and `From` impls.
- Re-exports of `ir_lang::BinOp` and `ir_lang::UnOp` so the operations carried by `Op` can be matched without depending on `ir-lang` directly.
- `serde` derives for the program types behind the `serde` feature.
- Integration tests covering the full codegen workflow (`double`, `abs`, `max`, a countdown loop) through a reference interpreter, invalid-input rejection tests, and property tests checking codegen against an independent evaluation.
- Criterion benchmarks for straight-line, branching, and looping functions.
- Full rustdoc with runnable examples on every public item; `docs/API.md` API reference.

### Changed

- Wired the `ir-lang` dependency (`ir-lang = "1"`), the IR this backend lowers, with `std` and `serde` forwarded through the crate's own features.

### Fixed

- Corrected the unparseable `keywords` and `categories` arrays in `Cargo.toml` that prevented the crate from building.
- Aligned `clippy.toml`'s `msrv` with the crate's declared `rust-version` (`1.85`).

---

## [0.1.0] - 2026-06-18

Initial scaffold and repository bootstrap. No domain logic yet &mdash; this release establishes the structure, tooling, and quality gates the implementation will be built on.

### Added

- `Cargo.toml` with crate metadata, Rust 2024 edition, MSRV 1.85.
- Dual `Apache-2.0 OR MIT` license files.
- `README.md`, `CHANGELOG.md`, and a documentation skeleton.
- `REPS.md` compliance baseline.
- `.github/workflows/ci.yml` CI matrix; `deny.toml`, `clippy.toml`, `rustfmt.toml`.
- `dev/ROADMAP.md` (committed plan).

[Unreleased]: https://github.com/jamesgober/codegen-lang/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/jamesgober/codegen-lang/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/jamesgober/codegen-lang/compare/v0.2.0...v1.0.0
[0.2.0]: https://github.com/jamesgober/codegen-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/codegen-lang/releases/tag/v0.1.0

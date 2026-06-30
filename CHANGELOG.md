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

[Unreleased]: https://github.com/jamesgober/codegen-lang/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/jamesgober/codegen-lang/compare/v0.2.0...v1.0.0
[0.2.0]: https://github.com/jamesgober/codegen-lang/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/codegen-lang/releases/tag/v0.1.0

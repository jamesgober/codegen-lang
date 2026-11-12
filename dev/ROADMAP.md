# codegen-lang - Roadmap

> Path from scaffold to a stable 1.0. Hard parts are front-loaded; each phase has hard exit criteria.
>
> **Anti-deferral rule:** no listed hard task moves to a later phase unless this file records the move and the reason.

## v0.1.0 - Scaffold (DONE)
Compiles, CI green, structure correct, no domain logic.
- [x] Manifest, README, CHANGELOG, REPS, dual license, CI, deny, clippy, rustfmt.

## v0.2.0 - Core (DONE)
A backend abstraction (`Backend` trait) that lowers IR to a target; the shipped target
is a register-based bytecode. `ir-lang` is wired here as the IR the backend reads.
Exit criteria:
- [x] Every public item has rustdoc + a runnable example.
- [x] Core invariants property-tested (full DIRECTIVES + API authored at this stage).

## v1.0.0 - API freeze (DONE)
Public surface stable and frozen until 2.0.
- [x] docs/API.md marked stable; SemVer promise recorded.
- [x] Full test + benchmark suite green on all three platforms.

## v1.0.1 - Patch: parallel-move fix (DONE, 2026-10-08)
Fixes ISSUES **H01** and the patch-level parts of **M51** from
`_lexersketch/ISSUES.md`. No public API change.

Delivered:
- **H01 (miscompile).** Block-argument moves are sequentialized as a parallel copy
  (`src/moves.rs`): ready-first ordering, one scratch register past the value registers
  to break each cycle, self-copies skipped, minimal move count (`k + 1` for a `k`-cycle),
  edges of up to 16 copies scheduled in a stack buffer (no heap), larger edges in linear
  time with side tables allocated once per function. Jumps and both branch
  arms share the scheduler. The false "SSA sources and destinations are disjoint" claim
  is removed from the README, `docs/API.md`, and the rustdoc.
- **M51 (patch parts).**
  - `Program::__check_invariants`, `#[doc(hidden)]` and outside the SemVer promise, so
    tests can check lowered and deserialized programs without a public API change.
  - Every unchecked `as u32` in the lowering replaced by `narrow` (checked, saturating,
    bound documented); no panic and no silent wrap.
  - `Display for Program` made `O(ops + labels·log labels)` with byte-identical output.
  - CFG/loop property tests (`tests/parallel_moves.rs`) checked against a direct IR
    interpreter (`tests/support/ir.rs`), plus named swap / rotation / shift /
    duplication regressions.

Dependency wiring: unchanged (`ir-lang = "1"`). `aot-lang` depends on
`codegen-lang = "1"` and picks the fix up with no change of its own.

Moved to v1.1.0 (anti-deferral record — moved because a patch release cannot add public
API, not because the work is hard):
- A public, documented `Program::validate` (the hidden check, promoted with a proper
  error type). Adding a public method is a minor change under the SemVer promise.
- Validating on `Deserialize` (rejecting a malformed `Program` at load). Rejecting input
  that 1.0.0 accepts is a behaviour change, so it goes with the public `validate` in a
  minor release, not a patch.
- A `CodegenError` variant for a function too large for `u32` numbering, so `narrow`'s
  saturating fallback becomes a reported error. A new variant is a minor change.

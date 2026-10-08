//! Sequentializing the parallel copy a control-flow edge performs.
//!
//! A block argument list is a *parallel* assignment: every argument is read before any
//! parameter is written, exactly like an SSA phi. Bytecode has only one-at-a-time
//! [`Move`](Op::Move)s, so the copies on an edge have to be put in an order that never
//! overwrites a register a later copy still has to read.
//!
//! The sources and destinations of one edge are *not* disjoint in general. A loop back
//! edge that passes its target's own parameters — `H(a, b): … jump H(b, a)` — reads the
//! very registers it writes, and a naive in-order copy turns the swap into `a ← b; b ← a`
//! (both end up holding `b`). The scheduler here is the standard parallel-move
//! sequentialization:
//!
//! - The destinations of one edge are distinct (they are the target block's parameters),
//!   so every register has at most one pending writer and the move graph is a set of
//!   trees hanging off at most one cycle each.
//! - A move is *ready* once no other pending move still reads its destination. Ready
//!   moves are emitted, and emitting one can release the move that writes its source.
//! - When nothing is ready but moves remain, every remaining move lies on a cycle. One
//!   cycle is broken by saving one move's source in a scratch register and redirecting
//!   that move to read the scratch instead; the rest of the cycle then drains as a chain.
//!
//! The emitted sequence is minimal: one move per copy whose source and destination
//! differ (self-copies are skipped) plus exactly one extra move per cycle, so a cycle of
//! length `k` costs `k + 1` moves. An edge of up to 16 copies (nearly every real edge) is
//! scheduled in a stack buffer with no heap use; a larger edge is scheduled in time linear
//! in its copy count, using side tables indexed by register that are allocated once per
//! function and left all-clear after each edge.

use alloc::vec::Vec;

use crate::program::{Op, Reg};

/// Marks a register with no pending writer in [`Mover::writer`].
const NO_WRITER: u32 = u32::MAX;

/// Edges with at most this many copies are scheduled in a stack buffer, without touching
/// the heap or the per-register tables. Real edges are almost always this small, and at
/// this size a direct scan of the buffer is cheaper than maintaining the tables. The
/// scan is quadratic in the copy count, so the bound keeps it a small constant.
const SMALL: usize = 16;

/// Schedules the argument copies of each control-flow edge of one function.
///
/// One instance serves every edge of a function. Small edges are scheduled on the stack.
/// Larger ones use per-register tables, sized to the function's value registers on first
/// use; each edge leaves them all-clear again, so they are allocated at most once.
pub(crate) struct Mover {
    /// The register used to break a cycle. It is one past the function's value
    /// registers, so it never aliases a value, and it is dead between edges, so a single
    /// scratch register serves every edge of the function.
    scratch: Reg,
    /// Whether any edge needed the scratch register; the program's register count grows
    /// by one exactly when it did.
    scratch_used: bool,
    /// Routes every edge through the table scheduler, so tests can exercise it on small
    /// edges too. Always `false` outside tests.
    force_tables: bool,
    /// The number of value registers, which sizes the side tables.
    value_registers: usize,
    /// Per register: how many pending moves of the current edge still read it.
    readers: Vec<u32>,
    /// Per register: the index of the pending move that writes it, or [`NO_WRITER`]. An
    /// entry is cleared as soon as its move is emitted, so a set entry means "pending".
    writer: Vec<u32>,
    /// A large edge's non-trivial moves as `(dst, src)`. A cycle break rewrites one
    /// entry's source to the scratch register.
    moves: Vec<(Reg, Reg)>,
    /// Moves of the current edge whose destination no pending move reads.
    ready: Vec<u32>,
}

impl Mover {
    /// Creates a scheduler for a function with `value_registers` value registers whose
    /// cycle-breaking scratch register is `scratch`.
    ///
    /// The caller passes `scratch` equal to `Reg(value_registers)`, so the scratch sits
    /// just past the value registers. Nothing is allocated until an edge carries more
    /// than [`SMALL`] copies, so a function without such an edge never touches the heap
    /// here.
    pub(crate) fn new(value_registers: usize, scratch: Reg) -> Self {
        Self {
            scratch,
            scratch_used: false,
            force_tables: false,
            value_registers,
            readers: Vec::new(),
            writer: Vec::new(),
            moves: Vec::new(),
            ready: Vec::new(),
        }
    }

    /// Whether any edge scheduled so far needed the scratch register to break a cycle.
    pub(crate) const fn scratch_used(&self) -> bool {
        self.scratch_used
    }

    /// Emits the moves that perform the parallel copy `dst[i] ← src[i]` for every pair,
    /// in an order that never overwrites a register a later move still reads.
    ///
    /// Every destination must be distinct (they are one block's parameters) and every
    /// register must be below the `value_registers` this scheduler was created with;
    /// the lowering only ever passes registers of a validated function, which satisfies
    /// both. A pair whose source equals its destination emits nothing.
    pub(crate) fn edge(&mut self, pairs: impl Iterator<Item = (Reg, Reg)>, ops: &mut Vec<Op>) {
        // Gather the non-trivial copies on the stack, spilling to `moves` only if the
        // edge outgrows the buffer.
        let mut buf = [(Reg(0), Reg(0)); SMALL];
        let mut len = 0;
        let mut spilled = false;
        for pair in pairs.filter(|&(dst, src)| dst != src) {
            if spilled {
                self.moves.push(pair);
            } else if len < SMALL {
                buf[len] = pair;
                len += 1;
            } else {
                self.moves.clear();
                self.moves.extend_from_slice(&buf);
                self.moves.push(pair);
                spilled = true;
            }
        }

        if !spilled && self.force_tables && len > 1 {
            self.moves.clear();
            self.moves.extend_from_slice(&buf[..len]);
            spilled = true;
        }
        if spilled {
            self.schedule(ops);
        } else {
            self.schedule_small(&mut buf[..len], ops);
        }
    }

    /// Schedules an edge of at most [`SMALL`] copies by scanning the buffer directly.
    ///
    /// Each pass emits, in argument order, every pending copy whose destination no
    /// pending copy still reads; passes repeat until none is emitted. Whatever is left
    /// then lies on cycles, and one cycle is broken through the scratch register. An
    /// edge with no conflicts is therefore emitted exactly in argument order, in one
    /// pass.
    fn schedule_small(&mut self, copies: &mut [(Reg, Reg)], ops: &mut Vec<Op>) {
        // Zero copies or one copy cannot conflict with itself. This is the shape of
        // almost every forward edge.
        if let [(dst, src)] = *copies {
            ops.push(Op::Move { dst, src });
            return;
        }
        // Bit `i` is set while copy `i` is pending. `copies.len() <= SMALL < 32`.
        let mut pending: u32 = (1_u32 << copies.len()) - 1;
        while pending != 0 {
            let mut progress = true;
            while progress {
                progress = false;
                for i in 0..copies.len() {
                    if pending & (1 << i) == 0 {
                        continue;
                    }
                    let (dst, src) = copies[i];
                    let blocked = copies
                        .iter()
                        .enumerate()
                        .any(|(j, &(_, read))| pending & (1 << j) != 0 && read == dst);
                    if !blocked {
                        ops.push(Op::Move { dst, src });
                        pending &= !(1 << i);
                        progress = true;
                    }
                }
            }
            if pending != 0 {
                // Every pending copy is on a cycle. Save the first one's source and let
                // it read the scratch instead; its cycle then drains as a chain.
                let first = pending.trailing_zeros() as usize;
                ops.push(Op::Move {
                    dst: self.scratch,
                    src: copies[first].1,
                });
                copies[first].1 = self.scratch;
                self.scratch_used = true;
            }
        }
    }

    /// Schedules a large edge (the copies in `moves`) with per-register tables, in time
    /// linear in its copy count.
    fn schedule(&mut self, ops: &mut Vec<Op>) {
        if self.readers.is_empty() {
            self.readers.resize(self.value_registers, 0);
            self.writer.resize(self.value_registers, NO_WRITER);
        }
        let count = self.moves.len();
        self.ready.clear();

        // Build the move graph: who reads each register, and which move writes it. Move
        // indices fit in `u32` because an edge carries at most one copy per target
        // parameter, and parameters are ir-lang values, numbered in `u32`.
        for (index, &(dst, src)) in self.moves.iter().enumerate() {
            self.readers[slot(src)] += 1;
            self.writer[slot(dst)] = narrow(index);
        }

        // Seed the ready set in reverse so the stack pops in argument order: an edge with
        // no conflicts is emitted exactly in the order its arguments were written.
        for index in (0..count).rev() {
            let (dst, _) = self.moves[index];
            if self.readers[slot(dst)] == 0 {
                self.ready.push(narrow(index));
            }
        }

        let mut cursor = 0;
        loop {
            self.drain_ready(ops);

            // Everything not yet emitted now lies on a cycle: each remaining destination
            // is read by exactly one remaining move. Find one (the cursor only moves
            // forward, so the scan is linear over the whole edge) and break its cycle. A
            // move is pending exactly while its destination still names it as writer.
            while cursor < count && self.writer[slot(self.moves[cursor].0)] == NO_WRITER {
                cursor += 1;
            }
            if cursor == count {
                break;
            }
            let (_, src) = self.moves[cursor];
            ops.push(Op::Move {
                dst: self.scratch,
                src,
            });
            self.scratch_used = true;
            self.moves[cursor].1 = self.scratch;
            // `src` is now saved, so the move that overwrites it may run.
            self.release(src);
        }
        // The tables are all-clear again: every writer entry was cleared as its move was
        // emitted, and every reader count fell back to zero as each move's original
        // source was released exactly once.
    }

    /// Emits every ready move, and every move each emission makes ready in turn.
    fn drain_ready(&mut self, ops: &mut Vec<Op>) {
        while let Some(index) = self.ready.pop() {
            let (dst, src) = self.moves[index as usize];
            ops.push(Op::Move { dst, src });
            self.writer[slot(dst)] = NO_WRITER;
            // A move redirected to the scratch register released its original source
            // when the cycle was broken; the scratch itself is not tracked.
            if src != self.scratch {
                self.release(src);
            }
        }
    }

    /// Records that one pending read of `src` is finished. When it was the last read,
    /// the move writing `src` (if any is pending) becomes ready.
    fn release(&mut self, src: Reg) {
        let readers = &mut self.readers[slot(src)];
        *readers -= 1;
        if *readers == 0 {
            let writer = self.writer[slot(src)];
            if writer != NO_WRITER {
                self.ready.push(writer);
            }
        }
    }
}

/// The side-table index of a value register.
#[inline]
fn slot(reg: Reg) -> usize {
    reg.0 as usize
}

/// Narrows an index or count to the `u32` the bytecode numbers registers, labels, and
/// op offsets in, without a panic and without wrapping.
///
/// # Bound
///
/// ir-lang numbers values and blocks with `u32` handles, so a register named after a
/// value and a label named after a block always fit. The other quantities narrowed
/// here — the register count (one past the highest value, plus the scratch register),
/// the internal labels a two-way branch appends past the block labels, an op offset,
/// and a move index within one edge — exceed `u32::MAX` only for a function whose
/// lowered program has more than `u32::MAX` registers, labels, or ops: billions of
/// entries and tens of gigabytes of IR. That is outside the range the bytecode format
/// can describe at all. Rather than wrap, which would silently alias a register or a
/// label, or panic, an out-of-range value saturates at `u32::MAX`.
#[inline]
pub(crate) fn narrow(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

#[cfg(test)]
#[allow(
    clippy::panic,
    reason = "tests check exact outcomes; a wrong one should fail the test loudly"
)]
mod tests {
    use super::Mover;
    use crate::program::{Op, Reg};
    use alloc::vec::Vec;
    use proptest::prelude::*;

    /// Runs `pairs` through a fresh scheduler over `registers` value registers, with the
    /// table scheduler forced or not, and returns the emitted ops and whether the
    /// scratch register was used.
    fn schedule_with(registers: u32, pairs: &[(u32, u32)], tables: bool) -> (Vec<Op>, bool) {
        let mut mover = Mover::new(registers as usize, Reg(registers));
        mover.force_tables = tables;
        let mut ops = Vec::new();
        mover.edge(pairs.iter().map(|&(d, s)| (Reg(d), Reg(s))), &mut ops);
        (ops, mover.scratch_used())
    }

    /// Runs `pairs` through both schedulers (the stack path the edge size selects, and
    /// the table path), asserts they agree on the result, the move count, and scratch
    /// use, and returns the default path's output.
    fn schedule(registers: u32, pairs: &[(u32, u32)]) -> (Vec<Op>, bool) {
        let (ops, scratch) = schedule_with(registers, pairs, false);
        let (table_ops, table_scratch) = schedule_with(registers, pairs, true);
        assert_eq!(execute(registers, &ops), execute(registers, &table_ops));
        assert_eq!(ops.len(), table_ops.len());
        assert_eq!(scratch, table_scratch);
        (ops, scratch)
    }

    /// Executes `ops` (moves only) over a register file where register `r` starts out
    /// holding `r`, with one extra slot for the scratch register.
    fn execute(registers: u32, ops: &[Op]) -> Vec<u32> {
        let mut file: Vec<u32> = (0..=registers).collect();
        for op in ops {
            match *op {
                Op::Move { dst, src } => file[dst.0 as usize] = file[src.0 as usize],
                other => panic!("the scheduler emitted a non-move op: {other:?}"),
            }
        }
        file.truncate(registers as usize);
        file
    }

    /// The reference: every source read first, then every destination written.
    fn parallel(registers: u32, pairs: &[(u32, u32)]) -> Vec<u32> {
        let mut file: Vec<u32> = (0..registers).collect();
        let reads: Vec<u32> = pairs.iter().map(|&(_, s)| file[s as usize]).collect();
        for (&(d, _), value) in pairs.iter().zip(reads) {
            file[d as usize] = value;
        }
        file
    }

    /// The minimal move count: one per non-self copy, plus one per cycle among them.
    fn minimal_moves(registers: u32, pairs: &[(u32, u32)]) -> usize {
        let mut source = alloc::vec![None; registers as usize];
        let live: Vec<(u32, u32)> = pairs.iter().copied().filter(|&(d, s)| d != s).collect();
        for &(d, s) in &live {
            source[d as usize] = Some(s);
        }
        // Walk back along sources from each destination; a walk that returns to its
        // start is a cycle, counted once at its smallest member.
        let mut cycles = 0;
        for &(start, _) in &live {
            let mut at = start;
            let mut smallest = start;
            let mut steps = 0;
            while let Some(s) = source[at as usize] {
                at = s;
                smallest = smallest.min(at);
                steps += 1;
                if at == start || steps > live.len() {
                    break;
                }
            }
            if at == start && smallest == start {
                cycles += 1;
            }
        }
        live.len() + cycles
    }

    fn check(registers: u32, pairs: &[(u32, u32)]) {
        let (ops, _) = schedule(registers, pairs);
        assert_eq!(execute(registers, &ops), parallel(registers, pairs));
        assert_eq!(ops.len(), minimal_moves(registers, pairs));
    }

    #[test]
    fn test_swap_uses_the_scratch_register() {
        let (ops, scratch) = schedule(2, &[(0, 1), (1, 0)]);
        assert!(scratch);
        assert_eq!(ops.len(), 3);
        assert_eq!(execute(2, &ops), [1, 0]);
    }

    #[test]
    fn test_rotation_costs_one_extra_move() {
        check(3, &[(0, 1), (1, 2), (2, 0)]);
        let (ops, _) = schedule(3, &[(0, 1), (1, 2), (2, 0)]);
        assert_eq!(ops.len(), 4);
    }

    #[test]
    fn test_duplication_needs_no_scratch() {
        // jump H(a, a) into H(a, b): b ← a, and a ← a is skipped.
        let (ops, scratch) = schedule(2, &[(0, 0), (1, 0)]);
        assert!(!scratch);
        assert_eq!(
            ops,
            [Op::Move {
                dst: Reg(1),
                src: Reg(0)
            }]
        );
    }

    #[test]
    fn test_chain_is_ordered_against_its_reads() {
        // a ← b, b ← c: b must be read before it is overwritten.
        check(3, &[(0, 1), (1, 2)]);
        // The issue's second shape: H(a, b) passed (a2, a): b ← a must precede a ← a2.
        check(3, &[(0, 2), (1, 0)]);
    }

    #[test]
    fn test_conflict_free_edge_keeps_argument_order() {
        let (ops, _) = schedule(6, &[(0, 3), (1, 4), (2, 5)]);
        assert_eq!(
            ops,
            [
                Op::Move {
                    dst: Reg(0),
                    src: Reg(3)
                },
                Op::Move {
                    dst: Reg(1),
                    src: Reg(4)
                },
                Op::Move {
                    dst: Reg(2),
                    src: Reg(5)
                },
            ]
        );
    }

    #[test]
    fn test_all_self_moves_emit_nothing() {
        let (ops, scratch) = schedule(3, &[(0, 0), (1, 1), (2, 2)]);
        assert!(ops.is_empty());
        assert!(!scratch);
    }

    #[test]
    fn test_tables_are_clean_between_edges() {
        // A second edge on the same scheduler must not see the first edge's graph.
        let mut mover = Mover::new(4, Reg(4));
        mover.force_tables = true;
        let mut ops = Vec::new();
        mover.edge([(Reg(0), Reg(1)), (Reg(1), Reg(0))].into_iter(), &mut ops);
        let first = ops.len();
        mover.edge([(Reg(2), Reg(3)), (Reg(3), Reg(2))].into_iter(), &mut ops);
        assert_eq!(first, 3);
        assert_eq!(execute(4, &ops), [1, 0, 3, 2]);
        assert!(mover.readers.iter().all(|&n| n == 0));
        assert!(mover.writer.iter().all(|&w| w == super::NO_WRITER));
    }

    #[test]
    fn test_stack_buffer_spills_at_its_boundary() {
        // Rotations of SMALL - 1, SMALL, SMALL + 1, and 2 * SMALL copies straddle the
        // switch from the stack buffer to the tables; each is one cycle.
        for n in [
            super::SMALL - 1,
            super::SMALL,
            super::SMALL + 1,
            2 * super::SMALL,
        ] {
            let n = n as u32;
            let pairs: Vec<(u32, u32)> = (0..n).map(|i| (i, (i + 1) % n)).collect();
            let (ops, scratch) = schedule(n, &pairs);
            assert!(scratch);
            assert_eq!(ops.len(), n as usize + 1);
            assert_eq!(execute(n, &ops), parallel(n, &pairs));
        }
    }

    #[test]
    fn test_long_cycle_is_linear_and_correct() {
        // A 100k-register rotation: one cycle, so n + 1 moves.
        let n = 100_000_u32;
        let pairs: Vec<(u32, u32)> = (0..n).map(|i| (i, (i + 1) % n)).collect();
        let (ops, scratch) = schedule(n, &pairs);
        assert!(scratch);
        assert_eq!(ops.len(), n as usize + 1);
        assert_eq!(execute(n, &ops), parallel(n, &pairs));
    }

    /// An edge: distinct destinations drawn from `0..registers`, each with an arbitrary
    /// source from the same range — permutations, rotations, duplicated sources, and
    /// self-copies all arise.
    fn edge_strategy() -> impl Strategy<Value = (u32, Vec<(u32, u32)>)> {
        (1_u32..40).prop_flat_map(|registers| {
            let dsts = Just((0..registers).collect::<Vec<u32>>()).prop_shuffle();
            let srcs = proptest::collection::vec(0..registers, registers as usize);
            (Just(registers), dsts, srcs, 0..=registers as usize).prop_map(
                |(registers, dsts, srcs, take)| {
                    let pairs = dsts.into_iter().zip(srcs).take(take).collect();
                    (registers, pairs)
                },
            )
        })
    }

    /// A full permutation of the registers: the all-cycles worst case.
    fn permutation_strategy() -> impl Strategy<Value = (u32, Vec<(u32, u32)>)> {
        (1_u32..40).prop_flat_map(|registers| {
            let srcs = Just((0..registers).collect::<Vec<u32>>()).prop_shuffle();
            (Just(registers), srcs).prop_map(|(registers, srcs)| {
                let pairs = (0..registers).zip(srcs).collect();
                (registers, pairs)
            })
        })
    }

    proptest! {
        #[test]
        fn scheduled_moves_equal_the_parallel_copy((registers, pairs) in edge_strategy()) {
            let (ops, _) = schedule(registers, &pairs);
            prop_assert_eq!(execute(registers, &ops), parallel(registers, &pairs));
        }

        #[test]
        fn scheduled_move_count_is_minimal((registers, pairs) in edge_strategy()) {
            let (ops, _) = schedule(registers, &pairs);
            prop_assert_eq!(ops.len(), minimal_moves(registers, &pairs));
        }

        #[test]
        fn permutations_are_sequentialized_exactly((registers, pairs) in permutation_strategy()) {
            check(registers, &pairs);
        }

        #[test]
        fn scratch_is_used_only_for_a_cycle((registers, pairs) in edge_strategy()) {
            let (_, scratch) = schedule(registers, &pairs);
            let live = pairs.iter().filter(|&&(d, s)| d != s).count();
            prop_assert_eq!(scratch, minimal_moves(registers, &pairs) > live);
        }
    }
}

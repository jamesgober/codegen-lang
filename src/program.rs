//! The compiled program and the bytecode it is made of.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use ir_lang::{BinOp, UnOp};

/// A virtual register: a numbered slot that holds one value while a program runs.
///
/// The bytecode is register-based rather than stack-based. Every value the source
/// function defines is given its own register, so an [`Op`] names its operands and its
/// result by register instead of by a position on an operand stack. When the argument
/// copies on some control-flow edge form a cycle (a loop passing its own parameters back
/// permuted), one extra scratch register past the value registers is used to break it.
/// Registers are dense from zero; [`Program::register_count`] is one past the highest in
/// use. A function's parameters occupy the first registers — read them from
/// [`Program::params`].
///
/// # Examples
///
/// ```
/// use codegen_lang::Reg;
///
/// let r = Reg(2);
/// assert_eq!(r.0, 2);
/// assert_eq!(r.to_string(), "r2");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Reg(pub u32);

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

/// A jump target: a position in a program's [op stream](Program::ops) that a
/// control-flow op transfers to.
///
/// Each basic block of the source function becomes a label, numbered by block index, so
/// the entry block is always [`Label(0)`](Program::entry). Laying out a two-way branch
/// needs one extra position for the second arm, so a backend appends a few internal
/// labels past the block labels. Resolve a label to an op index with
/// [`Program::label_offset`].
///
/// # Examples
///
/// ```
/// use codegen_lang::Label;
///
/// assert_eq!(Label(0).to_string(), "L0");
/// assert_eq!(Label(3).to_string(), "L3");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Label(pub u32);

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "L{}", self.0)
    }
}

/// A constant operand loaded by [`Op::Const`].
///
/// The three cases mirror the IR's three constant instructions
/// ([`Iconst`](ir_lang::Inst::Iconst), [`Fconst`](ir_lang::Inst::Fconst),
/// [`Bconst`](ir_lang::Inst::Bconst)) and carry the same payloads, so a constant is
/// reproduced exactly rather than widened or reinterpreted.
///
/// # Examples
///
/// ```
/// use codegen_lang::Const;
///
/// assert_eq!(Const::Int(-7).to_string(), "-7");
/// assert_eq!(Const::Bool(true).to_string(), "true");
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Const {
    /// A signed-integer constant.
    Int(i64),
    /// A floating-point constant.
    Float(f64),
    /// A boolean constant.
    Bool(bool),
}

impl fmt::Display for Const {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Const::Int(value) => write!(f, "{value}"),
            Const::Float(value) => write!(f, "{value}"),
            Const::Bool(value) => write!(f, "{value}"),
        }
    }
}

/// One bytecode instruction.
///
/// An op is the unit a [`Program`] is a sequence of. The arithmetic ops
/// ([`Const`](Op::Const), [`Bin`](Op::Bin), [`Un`](Op::Un)) write their result to a
/// destination [`Reg`] and read their operands from registers; [`Move`](Op::Move) copies
/// one register to another; and the control-flow ops ([`Jump`](Op::Jump),
/// [`JumpUnless`](Op::JumpUnless), [`Return`](Op::Return)) carry a [`Label`] or a result
/// register. The set is closed and every variant is `Copy`, so an op stream is a flat
/// `&[Op]` an interpreter or a further pass can walk with no indirection.
///
/// # Examples
///
/// ```
/// use codegen_lang::{Const, Op, Reg};
///
/// let load = Op::Const { dst: Reg(0), value: Const::Int(1) };
/// assert_eq!(load.to_string(), "r0 = const 1");
///
/// let ret = Op::Return { value: Some(Reg(0)) };
/// assert_eq!(ret.to_string(), "ret r0");
/// ```
#[derive(Clone, Copy, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum Op {
    /// Load a constant into `dst`.
    Const {
        /// Register the constant is written to.
        dst: Reg,
        /// The constant value.
        value: Const,
    },
    /// Apply a binary operation: `dst = lhs <op> rhs`.
    Bin {
        /// The operation, reusing the IR's [`BinOp`].
        op: BinOp,
        /// Register the result is written to.
        dst: Reg,
        /// Left operand register.
        lhs: Reg,
        /// Right operand register.
        rhs: Reg,
    },
    /// Apply a unary operation: `dst = <op> src`.
    Un {
        /// The operation, reusing the IR's [`UnOp`].
        op: UnOp,
        /// Register the result is written to.
        dst: Reg,
        /// Operand register.
        src: Reg,
    },
    /// Copy a register: `dst = src`. Emitted on a control-flow edge to move a block
    /// argument into the parameter register of the block being entered — the bytecode's
    /// stand-in for an SSA phi. The moves of one edge are ordered so that none
    /// overwrites a register a later one still reads; a cycle among them is broken by
    /// one extra move through a scratch register.
    Move {
        /// Destination register.
        dst: Reg,
        /// Source register.
        src: Reg,
    },
    /// Jump unconditionally to `target`.
    Jump {
        /// The label to continue at.
        target: Label,
    },
    /// Jump to `target` when `cond` holds `false`; otherwise fall through to the next op.
    JumpUnless {
        /// Register holding the boolean condition.
        cond: Reg,
        /// The label taken when the condition is `false`.
        target: Label,
    },
    /// Return from the function, optionally yielding the value in a register.
    Return {
        /// The register whose value is returned, or `None` for a unit return.
        value: Option<Reg>,
    },
}

impl fmt::Display for Op {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Op::Const { dst, value } => write!(f, "{dst} = const {value}"),
            Op::Bin { op, dst, lhs, rhs } => write!(f, "{dst} = {op} {lhs}, {rhs}"),
            Op::Un { op, dst, src } => write!(f, "{dst} = {op} {src}"),
            Op::Move { dst, src } => write!(f, "{dst} = {src}"),
            Op::Jump { target } => write!(f, "jump {target}"),
            Op::JumpUnless { cond, target } => write!(f, "jump_unless {cond}, {target}"),
            Op::Return { value: Some(reg) } => write!(f, "ret {reg}"),
            Op::Return { value: None } => write!(f, "ret"),
        }
    }
}

/// A lowered function: a flat bytecode program ready to be inspected, serialized, or run.
///
/// A program is produced by a [`Backend`](crate::Backend) — for the bytecode target, by
/// [`Bytecode`](crate::Bytecode) or the [`compile`](crate::compile) shortcut. It owns
/// the function's name, the registers holding its parameters, a count of every register
/// it uses, and the [op stream](Program::ops). Control-flow ops refer to positions in
/// that stream through [`Label`]s, which [`label_offset`](Program::label_offset)
/// resolves to op indices. Execution begins at the first op, the [entry](Program::entry)
/// block.
///
/// The [`Display`](fmt::Display) implementation renders the program as a readable
/// disassembly, which is the easiest way to see what a backend produced.
///
/// # Examples
///
/// ```
/// use codegen_lang::compile;
/// use ir_lang::{Builder, BinOp, Type};
///
/// // fn double(x: int) -> int { x + x }
/// let mut b = Builder::new("double", &[Type::Int], Type::Int);
/// let x = b.block_params(b.entry())[0];
/// let sum = b.bin(BinOp::Add, x, x);
/// b.ret(Some(sum));
/// let program = compile(&b.finish()).expect("double is well-formed");
///
/// assert_eq!(program.name(), "double");
/// assert_eq!(program.params().len(), 1);
/// assert_eq!(program.register_count(), 2); // x and the sum
/// assert!(!program.is_empty());
/// ```
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Program {
    pub(crate) name: String,
    pub(crate) params: Vec<Reg>,
    pub(crate) registers: u32,
    pub(crate) ops: Vec<Op>,
    pub(crate) labels: Vec<u32>,
}

impl Program {
    /// Returns the function's name.
    ///
    /// # Examples
    ///
    /// ```
    /// use codegen_lang::compile;
    /// use ir_lang::{Builder, Type};
    ///
    /// let mut b = Builder::new("main", &[], Type::Unit);
    /// b.ret(None);
    /// assert_eq!(compile(&b.finish()).unwrap().name(), "main");
    /// ```
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the registers holding the function's parameters, in declaration order.
    ///
    /// These are the registers an interpreter writes the call arguments into before it
    /// begins executing the [op stream](Program::ops).
    ///
    /// # Examples
    ///
    /// ```
    /// use codegen_lang::compile;
    /// use ir_lang::{Builder, Type};
    ///
    /// let mut b = Builder::new("f", &[Type::Int, Type::Bool], Type::Unit);
    /// b.ret(None);
    /// let program = compile(&b.finish()).unwrap();
    /// assert_eq!(program.params().len(), 2);
    /// ```
    #[must_use]
    pub fn params(&self) -> &[Reg] {
        &self.params
    }

    /// Returns the number of registers the program uses; valid register numbers are
    /// `0..register_count`.
    ///
    /// For a program lowered by [`Bytecode`](crate::Bytecode) this is the source
    /// function's value count, plus one when some edge needed the scratch register to
    /// break a cycle among its argument copies.
    ///
    /// # Examples
    ///
    /// ```
    /// use codegen_lang::compile;
    /// use ir_lang::{Builder, Type};
    ///
    /// let mut b = Builder::new("f", &[Type::Int], Type::Int);
    /// let x = b.block_params(b.entry())[0];
    /// let one = b.iconst(1);
    /// let r = b.bin(ir_lang::BinOp::Add, x, one);
    /// b.ret(Some(r));
    /// // x, the constant, and the sum.
    /// assert_eq!(compile(&b.finish()).unwrap().register_count(), 3);
    /// ```
    #[must_use]
    pub const fn register_count(&self) -> u32 {
        self.registers
    }

    /// Returns the program's ops, in execution order.
    ///
    /// # Examples
    ///
    /// ```
    /// use codegen_lang::{compile, Op};
    /// use ir_lang::{Builder, Type};
    ///
    /// let mut b = Builder::new("f", &[], Type::Unit);
    /// b.ret(None);
    /// let program = compile(&b.finish()).unwrap();
    /// assert!(matches!(program.ops(), [Op::Return { value: None }]));
    /// ```
    #[must_use]
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// Returns the number of ops in the program.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Returns `true` if the program has no ops. A program lowered from a valid function
    /// is never empty: its entry block always ends in a terminator op.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Resolves a label to the index of the op it points at, or `None` if the label does
    /// not belong to this program.
    ///
    /// # Examples
    ///
    /// ```
    /// use codegen_lang::compile;
    /// use ir_lang::{Builder, Type};
    ///
    /// let mut b = Builder::new("f", &[], Type::Unit);
    /// b.ret(None);
    /// let program = compile(&b.finish()).unwrap();
    /// // Execution starts at the entry label, which is the first op.
    /// assert_eq!(program.label_offset(program.entry()), Some(0));
    /// ```
    #[must_use]
    pub fn label_offset(&self, label: Label) -> Option<usize> {
        self.labels
            .get(label.0 as usize)
            .map(|&offset| offset as usize)
    }

    /// Returns the entry label, where execution begins: always `L0`, the source
    /// function's entry block, which lowers to the first op.
    #[must_use]
    pub const fn entry(&self) -> Label {
        Label(0)
    }
}

impl fmt::Display for Program {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}(", self.name)?;
        for (i, param) in self.params.iter().enumerate() {
            if i != 0 {
                f.write_str(", ")?;
            }
            write!(f, "{param}")?;
        }
        writeln!(f, ") regs={}", self.registers)?;

        // Pair each label with its offset, sort by offset (then id), and walk the pairs
        // alongside the ops. That costs `ops + labels·log(labels)` instead of scanning
        // every label for every op. The output is the same as that scan's for any
        // program, lowered or deserialized: labels sharing an offset print in id order,
        // and a label pointing past the last op never prints.
        let mut marks: Vec<(u32, usize)> = self
            .labels
            .iter()
            .enumerate()
            .map(|(id, &offset)| (offset, id))
            .collect();
        marks.sort_unstable();
        let mut marks = marks.into_iter().peekable();

        for (index, op) in self.ops.iter().enumerate() {
            while let Some(&(offset, id)) = marks.peek() {
                if offset as usize != index {
                    break;
                }
                marks.next();
                // Printed as the label's own `Display` would (`L{id}`), without narrowing
                // the vector index back into a `Label`.
                writeln!(f, "L{id}:")?;
            }
            writeln!(f, "    {op}")?;
        }
        Ok(())
    }
}

impl Program {
    /// Checks the structural invariants every program the [`Bytecode`](crate::Bytecode)
    /// backend emits upholds, and describes the first one violated.
    ///
    /// **Not part of the stable API.** This method is hidden from the documentation and
    /// exempt from the SemVer promise. It exists so tests — this crate's, and those of
    /// crates that consume a [`Program`] — can check a program, in particular one
    /// restored through `serde` (deserialization does not validate), without a public
    /// API change in a patch release. A public, documented validation is planned for a
    /// minor release; see `dev/ROADMAP.md`.
    ///
    /// The invariants, which together mean an interpreter can run the program without an
    /// out-of-range register, an unresolved jump, or running off the end of the ops:
    ///
    /// 1. There is at least one op and at least one label, and the entry label `L0`
    ///    resolves to op `0`.
    /// 2. Every register an op names, and every parameter register, is below
    ///    [`register_count`](Program::register_count); the parameter registers are
    ///    distinct.
    /// 3. Every label resolves to an op inside the stream, so every jump target does.
    /// 4. The last op is a [`Jump`](Op::Jump) or a [`Return`](Op::Return).
    ///
    /// Runs in `O(ops + labels + params·log(params))` and allocates only for the
    /// parameter-distinctness check.
    ///
    /// # Errors
    ///
    /// Returns a human-readable description naming the offending op, label, or register.
    #[doc(hidden)]
    pub fn __check_invariants(&self) -> Result<(), String> {
        let op_count = self.ops.len();
        let Some(last) = self.ops.last() else {
            return Err("the program has no ops".into());
        };
        match self.labels.first() {
            None => return Err("the program has no labels, so no entry label".into()),
            Some(&0) => {}
            Some(&offset) => {
                return Err(alloc::format!(
                    "the entry label L0 points at op {offset}, not op 0"
                ));
            }
        }

        // 3. Labels in range. Checking the table once covers every jump target that names
        // a label in it; a target outside the table is caught per op below.
        for (id, &offset) in self.labels.iter().enumerate() {
            if offset as usize >= op_count {
                return Err(alloc::format!(
                    "label L{id} points at op {offset}, past the last op ({op_count} ops)"
                ));
            }
        }

        // 2. Registers in range.
        let registers = self.registers;
        let in_range = |reg: Reg, at: usize| {
            if reg.0 < registers {
                Ok(())
            } else {
                Err(alloc::format!(
                    "op {at} names register {reg}, but the program has {registers} registers"
                ))
            }
        };
        let resolves = |target: Label, at: usize| {
            if (target.0 as usize) < self.labels.len() {
                Ok(())
            } else {
                Err(alloc::format!(
                    "op {at} jumps to {target}, which is not a label of the program"
                ))
            }
        };
        for (at, &op) in self.ops.iter().enumerate() {
            match op {
                Op::Const { dst, .. } => in_range(dst, at)?,
                Op::Bin { dst, lhs, rhs, .. } => {
                    in_range(dst, at)?;
                    in_range(lhs, at)?;
                    in_range(rhs, at)?;
                }
                Op::Un { dst, src, .. } | Op::Move { dst, src } => {
                    in_range(dst, at)?;
                    in_range(src, at)?;
                }
                Op::Jump { target } => resolves(target, at)?,
                Op::JumpUnless { cond, target } => {
                    in_range(cond, at)?;
                    resolves(target, at)?;
                }
                Op::Return { value } => {
                    if let Some(reg) = value {
                        in_range(reg, at)?;
                    }
                }
            }
        }

        let mut params = self.params.clone();
        params.sort_unstable();
        for pair in params.windows(2) {
            if pair[0] == pair[1] {
                return Err(alloc::format!("register {} is a parameter twice", pair[0]));
            }
        }
        if let Some(&highest) = params.last() {
            if highest.0 >= registers {
                return Err(alloc::format!(
                    "parameter register {highest} is out of range ({registers} registers)"
                ));
            }
        }

        // 4. No fall-through off the end.
        if !matches!(last, Op::Jump { .. } | Op::Return { .. }) {
            return Err(alloc::format!(
                "the last op ({last}) neither jumps nor returns, so execution runs off the end"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests build known-valid functions, so compilation cannot fail"
)]
mod tests {
    use super::{Const, Label, Op, Program, Reg};
    use crate::compile;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;
    use ir_lang::{BinOp, Builder, Type};

    #[test]
    fn test_reg_and_label_display_use_short_prefixes() {
        assert_eq!(Reg(0).to_string(), "r0");
        assert_eq!(Reg(41).to_string(), "r41");
        assert_eq!(Label(0).to_string(), "L0");
    }

    #[test]
    fn test_const_display_matches_payload() {
        assert_eq!(Const::Int(5).to_string(), "5");
        assert_eq!(Const::Int(-5).to_string(), "-5");
        assert_eq!(Const::Bool(false).to_string(), "false");
    }

    #[test]
    fn test_op_display_renders_each_form() {
        assert_eq!(
            Op::Const {
                dst: Reg(0),
                value: Const::Int(3)
            }
            .to_string(),
            "r0 = const 3"
        );
        assert_eq!(
            Op::Bin {
                op: BinOp::Add,
                dst: Reg(2),
                lhs: Reg(0),
                rhs: Reg(1)
            }
            .to_string(),
            "r2 = add r0, r1"
        );
        assert_eq!(
            Op::Move {
                dst: Reg(1),
                src: Reg(0)
            }
            .to_string(),
            "r1 = r0"
        );
        assert_eq!(Op::Jump { target: Label(1) }.to_string(), "jump L1");
        assert_eq!(
            Op::JumpUnless {
                cond: Reg(0),
                target: Label(2)
            }
            .to_string(),
            "jump_unless r0, L2"
        );
        assert_eq!(Op::Return { value: None }.to_string(), "ret");
    }

    #[test]
    fn test_program_disassembly_prints_header_label_and_ops() {
        let mut b = Builder::new("double", &[Type::Int], Type::Int);
        let x = b.block_params(b.entry())[0];
        let sum = b.bin(BinOp::Add, x, x);
        b.ret(Some(sum));
        let text = compile(&b.finish()).unwrap().to_string();

        assert!(text.starts_with("double(r0) regs=2"));
        assert!(text.contains("L0:"));
        assert!(text.contains("r1 = add r0, r0"));
        assert!(text.contains("ret r1"));
        // The exact disassembly, as documented in the API reference and release note.
        assert_eq!(
            text,
            "double(r0) regs=2\nL0:\n    r1 = add r0, r0\n    ret r1\n"
        );
    }

    #[test]
    fn test_entry_label_resolves_to_first_op() {
        let mut b = Builder::new("f", &[], Type::Unit);
        b.ret(None);
        let program = compile(&b.finish()).unwrap();
        assert_eq!(program.entry(), Label(0));
        assert_eq!(program.label_offset(program.entry()), Some(0));
        assert_eq!(program.label_offset(Label(999)), None);
    }

    /// A hand-built program, as a deserialized one could be.
    fn raw(registers: u32, params: Vec<Reg>, ops: Vec<Op>, labels: Vec<u32>) -> Program {
        Program {
            name: String::from("raw"),
            params,
            registers,
            ops,
            labels,
        }
    }

    /// The 1.0.0 disassembly loop, kept as the reference the faster one must match.
    fn reference_disassembly(program: &Program) -> String {
        use core::fmt::Write;
        let mut out = String::new();
        write!(out, "{}(", program.name).unwrap();
        for (i, param) in program.params.iter().enumerate() {
            if i != 0 {
                out.push_str(", ");
            }
            write!(out, "{param}").unwrap();
        }
        writeln!(out, ") regs={}", program.registers).unwrap();
        for (index, op) in program.ops.iter().enumerate() {
            for (id, &offset) in program.labels.iter().enumerate() {
                if offset as usize == index {
                    writeln!(out, "L{id}:").unwrap();
                }
            }
            writeln!(out, "    {op}").unwrap();
        }
        out
    }

    #[test]
    fn test_disassembly_matches_the_per_op_label_scan() {
        let ret = Op::Return { value: None };
        let jump = Op::Jump { target: Label(0) };
        // Shared offsets, unsorted ids, and a label past the end (never printed).
        let odd = raw(
            0,
            Vec::new(),
            alloc::vec![jump, jump, ret],
            alloc::vec![2, 0, 2, 9, 1],
        );
        assert_eq!(odd.to_string(), reference_disassembly(&odd));
        assert_eq!(
            odd.to_string(),
            "raw() regs=0\nL1:\n    jump L0\nL4:\n    jump L0\nL0:\nL2:\n    ret\n"
        );

        // A lowered program with a loop and a branch.
        let mut b = Builder::new("loop", &[Type::Int], Type::Int);
        let n = b.block_params(b.entry())[0];
        let header = b.create_block(&[Type::Int]);
        let exit = b.create_block(&[]);
        b.jump(header, &[n]);
        b.switch_to(header);
        let i = b.block_params(header)[0];
        let zero = b.iconst(0);
        let more = b.bin(BinOp::Gt, i, zero);
        let one = b.iconst(1);
        let next = b.bin(BinOp::Sub, i, one);
        b.branch(more, header, &[next], exit, &[]);
        b.switch_to(exit);
        b.ret(Some(i));
        let lowered = compile(&b.finish()).unwrap();
        assert_eq!(lowered.to_string(), reference_disassembly(&lowered));
    }

    #[test]
    fn test_lowered_programs_pass_the_invariant_check() {
        let mut b = Builder::new("double", &[Type::Int], Type::Int);
        let x = b.block_params(b.entry())[0];
        let sum = b.bin(BinOp::Add, x, x);
        b.ret(Some(sum));
        assert_eq!(compile(&b.finish()).unwrap().__check_invariants(), Ok(()));
    }

    #[test]
    fn test_invariant_check_reports_each_violation() {
        let ret = |r: u32| Op::Return {
            value: Some(Reg(r)),
        };
        let fails = |p: Program| p.__check_invariants().unwrap_err();

        let empty = raw(1, Vec::new(), Vec::new(), alloc::vec![0]);
        assert!(fails(empty).contains("no ops"));
        let unlabeled = raw(1, Vec::new(), alloc::vec![ret(0)], Vec::new());
        assert!(fails(unlabeled).contains("no labels"));
        let late_entry = raw(1, Vec::new(), alloc::vec![ret(0), ret(0)], alloc::vec![1]);
        assert!(fails(late_entry).contains("L0"));
        let past_end = raw(1, Vec::new(), alloc::vec![ret(0)], alloc::vec![0, 1]);
        assert!(fails(past_end).contains("L1"));
        let high_reg = raw(1, Vec::new(), alloc::vec![ret(3)], alloc::vec![0]);
        assert!(fails(high_reg).contains("r3"));
        let bad_jump = Op::Jump { target: Label(5) };
        let unresolved = raw(1, Vec::new(), alloc::vec![bad_jump], alloc::vec![0]);
        assert!(fails(unresolved).contains("L5"));
        let bad_cond = Op::JumpUnless {
            cond: Reg(2),
            target: Label(0),
        };
        let cond_range = raw(1, Vec::new(), alloc::vec![bad_cond, ret(0)], alloc::vec![0]);
        assert!(fails(cond_range).contains("r2"));
        let twice = raw(
            1,
            alloc::vec![Reg(0), Reg(0)],
            alloc::vec![ret(0)],
            alloc::vec![0],
        );
        assert!(fails(twice).contains("twice"));
        let high_param = raw(1, alloc::vec![Reg(4)], alloc::vec![ret(0)], alloc::vec![0]);
        assert!(fails(high_param).contains("r4"));
        let copy = Op::Move {
            dst: Reg(0),
            src: Reg(0),
        };
        let falls = raw(1, Vec::new(), alloc::vec![copy], alloc::vec![0]);
        assert!(fails(falls).contains("runs off the end"));
    }
}

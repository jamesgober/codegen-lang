//! The compiled program and the bytecode it is made of.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use ir_lang::{BinOp, UnOp};

/// A virtual register: a numbered slot that holds one value while a program runs.
///
/// The bytecode is register-based rather than stack-based. Every value the source
/// function defines is given its own register, so an [`Op`] names its operands and its
/// result by register instead of by a position on an operand stack. Registers are dense
/// from zero; [`Program::register_count`] is one past the highest in use. A function's
/// parameters occupy the first registers — read them from [`Program::params`].
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
    /// stand-in for an SSA phi.
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

        for (index, op) in self.ops.iter().enumerate() {
            // A label points at exactly one op offset and no two labels share one, so at
            // most one label prints before each op.
            for (id, &offset) in self.labels.iter().enumerate() {
                if offset as usize == index {
                    writeln!(f, "{}:", Label(id as u32))?;
                }
            }
            writeln!(f, "    {op}")?;
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
    use super::{Const, Label, Op, Reg};
    use crate::compile;
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
}

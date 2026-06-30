//! The [`Backend`] abstraction and the [`Bytecode`] reference backend.

use ir_lang::Function;

use crate::error::CodegenError;
use crate::lower::lower;
use crate::program::Program;

/// A code generator: lowers a function in SSA form to a concrete target representation.
///
/// This is the abstraction the crate is built around. A backend reads the IR a
/// front-end produced and turns it into the form a particular target consumes. The
/// [`Bytecode`] backend shipped here emits a flat [`Program`]; a backend layered on a
/// native code generator later — LLVM, Cranelift — produces that generator's own module
/// type, named by the [`Output`](Backend::Output) associated type. Drawing the boundary
/// as a trait is what lets a front-end be written against codegen without committing to
/// any one target.
///
/// A backend does not own the function and keeps no per-call state, so a single instance
/// compiles many functions and is cheap to pass around and share.
///
/// # Implementing a backend
///
/// A backend's `compile` is responsible for rejecting input it cannot lower. The
/// convention the bytecode backend follows is to call
/// [`Function::validate`](ir_lang::Function::validate) first and return
/// [`CodegenError::InvalidIr`] on failure, so the lowering proper only ever runs on
/// well-formed SSA.
///
/// # Examples
///
/// Compile with the shipped backend through the trait:
///
/// ```
/// use codegen_lang::{Backend, Bytecode};
/// use ir_lang::{Builder, Type};
///
/// let mut b = Builder::new("noop", &[], Type::Unit);
/// b.ret(None);
///
/// let program = Bytecode.compile(&b.finish()).expect("noop is well-formed");
/// assert_eq!(program.name(), "noop");
/// ```
pub trait Backend {
    /// The representation this backend emits.
    type Output;

    /// Lowers `func` to this backend's [`Output`](Backend::Output).
    ///
    /// # Errors
    ///
    /// Returns [`CodegenError::InvalidIr`] when `func` is not well-formed SSA, and any
    /// target-specific failure a particular backend defines.
    fn compile(&self, func: &Function) -> Result<Self::Output, CodegenError>;
}

/// The reference backend: lowers a function to a flat, register-based [`Program`].
///
/// `Bytecode` is the concrete target shipped with the crate and the one
/// [`compile`] uses. It validates the function, then lowers each block in order to a
/// linear op stream — small enough to read, serialize, and execute, which makes it the
/// natural target for testing a front-end's output before a native backend exists.
///
/// It is a zero-sized type with no configuration; construct it directly or with
/// [`Default`].
///
/// # Examples
///
/// ```
/// use codegen_lang::{Backend, Bytecode};
/// use ir_lang::{Builder, BinOp, Type};
///
/// // fn inc(x: int) -> int { x + 1 }
/// let mut b = Builder::new("inc", &[Type::Int], Type::Int);
/// let x = b.block_params(b.entry())[0];
/// let one = b.iconst(1);
/// let sum = b.bin(BinOp::Add, x, one);
/// b.ret(Some(sum));
///
/// let program = Bytecode.compile(&b.finish()).expect("inc is well-formed");
/// assert_eq!(program.register_count(), 3); // x, the constant, the sum
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Bytecode;

impl Backend for Bytecode {
    type Output = Program;

    fn compile(&self, func: &Function) -> Result<Program, CodegenError> {
        func.validate().map_err(CodegenError::InvalidIr)?;
        Ok(lower(func))
    }
}

/// Lowers a function to bytecode with the default [`Bytecode`] backend.
///
/// A shortcut for `Bytecode.compile(func)` for the common case where the bytecode target
/// is the one you want. Use [`Bytecode`] through the [`Backend`] trait when a generic
/// over backends is what you need.
///
/// # Errors
///
/// Returns [`CodegenError::InvalidIr`] if `func` is not well-formed SSA.
///
/// # Examples
///
/// ```
/// use codegen_lang::compile;
/// use ir_lang::{Builder, Type, UnOp};
///
/// // fn negate(x: int) -> int { -x }
/// let mut b = Builder::new("negate", &[Type::Int], Type::Int);
/// let x = b.block_params(b.entry())[0];
/// let neg = b.un(UnOp::Neg, x);
/// b.ret(Some(neg));
///
/// let program = compile(&b.finish()).expect("negate is well-formed");
/// assert_eq!(program.name(), "negate");
/// ```
pub fn compile(func: &Function) -> Result<Program, CodegenError> {
    Bytecode.compile(func)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "tests build known-valid functions, so compilation cannot fail"
)]
mod tests {
    use super::{Backend, Bytecode, compile};
    use ir_lang::{Builder, Type};

    #[test]
    fn test_compile_shortcut_matches_the_backend() {
        let func = {
            let mut b = Builder::new("f", &[Type::Int], Type::Int);
            let x = b.block_params(b.entry())[0];
            b.ret(Some(x));
            b.finish()
        };
        assert_eq!(compile(&func).unwrap(), Bytecode.compile(&func).unwrap());
    }

    #[test]
    fn test_backend_is_zero_sized_and_reusable() {
        assert_eq!(core::mem::size_of::<Bytecode>(), 0);

        let backend = Bytecode;
        let mut first = Builder::new("a", &[], Type::Unit);
        first.ret(None);
        let mut second = Builder::new("b", &[], Type::Unit);
        second.ret(None);

        // One instance compiles many functions.
        assert_eq!(backend.compile(&first.finish()).unwrap().name(), "a");
        assert_eq!(backend.compile(&second.finish()).unwrap().name(), "b");
    }
}

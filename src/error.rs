//! The error a backend reports when it cannot lower a function.

use core::fmt;

use ir_lang::ValidationError;

/// The reason a [`Backend`](crate::Backend) could not lower a function.
///
/// Lowering needs well-formed SSA: every value defined before it is used, every branch
/// targeting a real block with arguments that match its parameters, every operation
/// applied to operands of the right type. A backend checks that up front with
/// [`Function::validate`](ir_lang::Function::validate) and refuses to emit code for
/// input that fails, rather than producing a program that is wrong in a way the IR
/// already forbids.
///
/// The set is `#[non_exhaustive]`: a backend that performs target-specific checks may
/// report failures the bytecode backend never does, so a `match` on this type must
/// include a wildcard arm.
///
/// # Examples
///
/// ```
/// use codegen_lang::{compile, CodegenError};
/// use ir_lang::{Builder, Type};
///
/// // A function whose entry block never receives a terminator is not well-formed.
/// let func = Builder::new("f", &[], Type::Unit).finish();
/// match compile(&func) {
///     Err(CodegenError::InvalidIr(reason)) => {
///         assert!(reason.to_string().contains("terminator"));
///     }
///     other => panic!("expected an InvalidIr error, got {other:?}"),
/// }
/// ```
#[derive(Clone, PartialEq, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[non_exhaustive]
pub enum CodegenError {
    /// The input function did not pass
    /// [`Function::validate`](ir_lang::Function::validate). The wrapped
    /// [`ValidationError`] names the offending block or value and explains the
    /// violation; fix the lowering that produced the IR and try again.
    InvalidIr(ValidationError),
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::InvalidIr(reason) => {
                write!(
                    f,
                    "cannot lower a function that is not well-formed: {reason}"
                )
            }
        }
    }
}

impl core::error::Error for CodegenError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            CodegenError::InvalidIr(reason) => Some(reason),
        }
    }
}

impl From<ValidationError> for CodegenError {
    fn from(reason: ValidationError) -> Self {
        CodegenError::InvalidIr(reason)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests assert on specific outcomes; a wrong outcome should fail the test loudly"
)]
mod tests {
    use super::CodegenError;
    use crate::compile;
    use ir_lang::{Builder, Type, ValidationError};

    #[test]
    fn test_invalid_ir_error_wraps_the_validation_reason() {
        let func = Builder::new("f", &[], Type::Unit).finish();
        let err = match compile(&func) {
            Err(e) => e,
            Ok(_) => panic!("an unterminated function must not compile"),
        };
        assert!(matches!(
            err,
            CodegenError::InvalidIr(ValidationError::MissingTerminator { .. })
        ));
    }

    #[test]
    fn test_display_carries_the_underlying_reason() {
        let func = Builder::new("f", &[], Type::Int).finish();
        let err = compile(&func).expect_err("non-unit function with no return is invalid");
        let text = err.to_string();
        assert!(text.starts_with("cannot lower a function that is not well-formed"));
    }

    #[test]
    fn test_error_source_is_the_validation_error() {
        use core::error::Error;
        let func = Builder::new("f", &[], Type::Unit).finish();
        let err = compile(&func).expect_err("unterminated function is invalid");
        assert!(err.source().is_some());
    }

    #[test]
    fn test_from_validation_error_constructs_invalid_ir() {
        let func = Builder::new("f", &[], Type::Unit).finish();
        let reason = func
            .validate()
            .expect_err("unterminated function is invalid");
        let err: CodegenError = reason.into();
        assert!(matches!(err, CodegenError::InvalidIr(_)));
    }
}

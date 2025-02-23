//! Errors for the `brisc-isa` crate.

use thiserror::Error;

/// An error that occurs when decoding an [Instruction].
///
/// [Instruction]: crate::Instruction
#[derive(Error, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionDecodeError {
    /// Invalid opcode.
    #[error("Invalid opcode: {0:07b}")]
    InvalidOpcode(u8),
    /// Invalid function qualifiers.
    #[error("Invalid function qualifiers: {q_b:03b} and {q_b:07b}")]
    InvalidFunction {
        /// First function qualifier.
        q_a: u8,
        /// Second function qualifier.
        q_b: u8,
    },
}

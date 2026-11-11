//! Errors for the `brisc-isa` crate.

use thiserror::Error;

/// An error that occurs when decoding an [Instruction].
///
/// [Instruction]: crate::Instruction
#[derive(Error, Clone, Copy, Debug, PartialEq, Eq)]
pub enum InstructionDecodeError {
    /// No instruction of the enabled instruction sets has this opcode: the low seven bits of a
    /// 32-bit instruction or the low two bits of a 16-bit one.
    #[error("Invalid opcode: {0:07b}")]
    InvalidOpcode(u8),
    /// The fields that select an instruction among those with the same opcode select none.
    #[error("Invalid function qualifiers: {q_a:03b} and {q_b:07b}")]
    InvalidFunction {
        /// First function qualifier, usually funct3.
        q_a: u8,
        /// Second function qualifier, usually funct7 or zero.
        q_b: u8,
    },
    /// The encoding is one the instruction set reserves: an operand has a value that the
    /// instruction does not allow.
    #[error("Reserved encoding")]
    Reserved,
}

//! Errors for the `brisc-hw` crate.

use crate::memory::MemoryError;
use brisc_isa::{InstructionDecodeError, XWord};
use thiserror::Error;

/// An error that occurs while executing the pipeline.
#[derive(Error, Debug)]
pub enum PipelineError {
    /// A field is missing in the pipeline state.
    #[error("Missing Pipeline State: {0}")]
    MissingState(&'static str),
    /// An error occurred while decoding an instruction.
    #[error(transparent)]
    InstructionDecodeError(#[from] InstructionDecodeError),
    /// An error occurred in the memory bus.
    #[error("{0}")]
    MemoryError(MemoryError),
    /// A syscall exception occurred.
    #[error("Syscall exception occurred. Syscall number: {0}")]
    SyscallException(XWord),
    /// Bad AMO size detected in atomic instruction.
    #[cfg(feature = "a")]
    #[error("Bad AMO size: {0}")]
    BadAmoSize(u8),
    /// Unaligned atomic memory access.
    #[cfg(feature = "a")]
    #[error("Unaligned atomic memory access.")]
    UnalignedAmo,
}

/// A [Result] type with [Result::Err] = [PipelineError].
pub type PipelineResult<T> = Result<T, PipelineError>;

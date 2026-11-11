//! Why the processor stops running.

use brisc_isa::InstructionDecodeError;
use thiserror::Error;

/// The reason [Cpu::run] returned.
///
/// [Cpu::run]: super::Cpu::run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The instruction at the program counter is an `ecall`.
    ///
    /// It counts as retired, and the program counter still points at it: to continue after
    /// servicing the call, advance [Cpu::pc] by four bytes.
    ///
    /// [Cpu::pc]: super::Cpu::pc
    Ecall,
    /// The requested number of instructions has been retired.
    Budget,
    /// The instruction at the program counter cannot be executed. It changed no state.
    Fault(Fault),
}

/// A condition that prevents an instruction from executing.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The word at the program counter is not an instruction.
    #[error(transparent)]
    Decode(#[from] InstructionDecodeError),
    /// The address of an atomic instruction is not aligned to its access size.
    #[cfg(feature = "a")]
    #[error("Unaligned atomic memory access.")]
    UnalignedAmo,
}

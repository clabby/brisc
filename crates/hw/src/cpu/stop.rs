//! Why the processor stops running.

use brisc_isa::InstructionDecodeError;
use core::convert::Infallible;
use thiserror::Error;

/// The reason [Cpu::run] returned.
///
/// `E` is the error of the bus that the hart ran on.
///
/// [Cpu::run]: super::Cpu::run
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop<E = Infallible> {
    /// The instruction at the program counter is an `ecall`.
    ///
    /// It counts as retired, and the program counter still points at it: to continue after
    /// servicing the call, advance [Cpu::pc] by four bytes.
    ///
    /// [Cpu::pc]: super::Cpu::pc
    Ecall,
    /// The instruction at the program counter is an `ebreak`.
    ///
    /// It has not been retired and has changed no state. To continue after it, advance
    /// [Cpu::pc] by the length of the instruction: two bytes if it is a `c.ebreak`, four
    /// otherwise.
    ///
    /// [Cpu::pc]: super::Cpu::pc
    Ebreak,
    /// The requested number of instructions has been retired.
    Budget,
    /// The instruction at the program counter cannot be executed. It changed no state.
    Fault(Fault),
    /// The bus failed an access that the instruction at the program counter needs: the fetch of
    /// the instruction itself, a load or a store. The instruction changed no register.
    Bus(E),
}

/// A condition that prevents an instruction from executing.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The word at the program counter is not an instruction.
    #[error(transparent)]
    Decode(#[from] InstructionDecodeError),
    /// The instruction writes to a control and status register that is read-only: one whose
    /// address, given here, has its two highest bits set.
    #[error("Write to the read-only CSR {0:#05x}")]
    ReadOnlyCsr(u16),
    /// The address of an atomic instruction is not aligned to its access size.
    #[cfg(feature = "a")]
    #[error("Unaligned atomic memory access.")]
    UnalignedAmo,
}

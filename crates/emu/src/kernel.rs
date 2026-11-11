//! The interface to the environment that services a guest's system calls.
//!
//! The processor never calls a kernel. [Cpu::run] returns [Stop::Ecall] when the guest makes a
//! system call, and whoever drives the processor hands the guest's registers and its bus to a
//! [Kernel] or an [AsyncKernel] before running it again.
//!
//! [Cpu::run]: brisc_hw::cpu::Cpu::run
//! [Stop::Ecall]: brisc_hw::cpu::Stop::Ecall

use brisc_hw::{
    bus::{Address, Bus},
    cpu::Registers,
    XWord, REG_A0, REG_A1, REG_A2,
};
use core::future::{ready, Future};
use thiserror::Error;

/// What the guest does after a system call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    /// The guest continues with the instruction after the `ecall`.
    Continue,
    /// The guest exits with this exit code.
    Exit(XWord),
}

/// The reason a system call was not serviced.
///
/// `K` is the error of the kernel and `B` the error of the bus. A kernel can apply `?` to the
/// result of an access to the bus, because an error of the bus converts to
/// [SyscallError::Bus].
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError<K, B> {
    /// The kernel cannot service the system call.
    #[error("the kernel cannot service the system call")]
    Kernel(K),
    /// The bus failed an access that the kernel made.
    #[error("the bus failed an access by the kernel")]
    Bus(B),
}

impl<K, B> From<B> for SyscallError<K, B> {
    fn from(error: B) -> Self {
        Self::Bus(error)
    }
}

/// A kernel that services system calls synchronously.
///
/// `S` is state that lives outside the kernel and is passed to every system call, for the kernel
/// to read and modify.
pub trait Kernel<S> {
    /// The error returned when a system call cannot be serviced.
    type Error;

    /// Services system call number `syscall_no`. Its arguments are in `registers` and on the
    /// `bus`, which also receive its results.
    fn syscall<B: Bus>(
        &mut self,
        syscall_no: XWord,
        bus: &mut B,
        registers: &mut Registers,
        state: &mut S,
    ) -> Result<Syscall, SyscallError<Self::Error, B::Error>>;
}

/// A kernel that may suspend while it services a system call.
///
/// `S` is state that lives outside the kernel and is passed to every system call, for the kernel
/// to read and modify. Every [Kernel] is also an [AsyncKernel] that never suspends.
pub trait AsyncKernel<S> {
    /// The error returned when a system call cannot be serviced.
    type Error;

    /// Services system call number `syscall_no`. Its arguments are in `registers` and on the
    /// `bus`, which also receive its results.
    fn syscall<B: Bus>(
        &mut self,
        syscall_no: XWord,
        bus: &mut B,
        registers: &mut Registers,
        state: &mut S,
    ) -> impl Future<Output = Result<Syscall, SyscallError<Self::Error, B::Error>>>;
}

impl<S, K: Kernel<S>> AsyncKernel<S> for K {
    type Error = K::Error;

    fn syscall<B: Bus>(
        &mut self,
        syscall_no: XWord,
        bus: &mut B,
        registers: &mut Registers,
        state: &mut S,
    ) -> impl Future<Output = Result<Syscall, SyscallError<Self::Error, B::Error>>> {
        ready(Kernel::syscall(self, syscall_no, bus, registers, state))
    }
}

/// The kernel of a guest that makes no system calls: every system call is an error.
impl<S> Kernel<S> for () {
    type Error = ();

    fn syscall<B: Bus>(
        &mut self,
        _: XWord,
        _: &mut B,
        _: &mut Registers,
        _: &mut S,
    ) -> Result<Syscall, SyscallError<Self::Error, B::Error>> {
        Err(SyscallError::Kernel(()))
    }
}

/// A kernel with the two system calls that a program without an operating system needs, under
/// the numbers that Linux gives them on RISC-V.
///
/// - [MinimalKernel::EXIT] ends the program with the exit code in `a0`.
/// - [MinimalKernel::WRITE] appends the `a2` bytes at the address in `a1` to the state of the
///   kernel, whatever file descriptor `a0` names, and returns their number in `a0`.
///
/// The state can be any collection of bytes, such as a `Vec<u8>`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MinimalKernel;

impl MinimalKernel {
    /// The number of the `write` system call.
    pub const WRITE: XWord = 64;

    /// The number of the `exit` system call.
    pub const EXIT: XWord = 93;
}

/// The number of a system call that the kernel does not implement.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
#[error("unknown system call {0}")]
pub struct UnknownSyscall(pub XWord);

impl<S: Extend<u8>> Kernel<S> for MinimalKernel {
    type Error = UnknownSyscall;

    fn syscall<B: Bus>(
        &mut self,
        syscall_no: XWord,
        bus: &mut B,
        registers: &mut Registers,
        output: &mut S,
    ) -> Result<Syscall, SyscallError<Self::Error, B::Error>> {
        match syscall_no {
            Self::EXIT => Ok(Syscall::Exit(registers.get(REG_A0))),
            Self::WRITE => {
                let len = registers.get(REG_A2);
                let mut address = registers.get(REG_A1);
                let mut remaining = len;
                // The bytes pass through a buffer on the stack, a piece at a time.
                let mut buffer = [0; 256];
                while remaining > 0 {
                    let piece = &mut buffer[..remaining.min(256) as usize];
                    bus.read(address, piece)?;
                    output.extend(piece.iter().copied());
                    address = address.wrapping_add(piece.len() as Address);
                    remaining -= piece.len() as XWord;
                }
                registers.set(REG_A0, len);
                Ok(Syscall::Continue)
            }
            _ => Err(SyscallError::Kernel(UnknownSyscall(syscall_no))),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{EmulationError, Emulator};
    use brisc_hw::{memory::SparseMemory, rv_code};

    #[test]
    fn minimal_kernel_writes_and_exits() {
        // More bytes than the kernel moves at a time.
        let message = (0..300).map(|i| i as u8).collect::<Vec<_>>();
        let code = rv_code! {
            addi a0, zero, 1;
            addi a1, zero, 0x700;
            addi a2, zero, 300;
            addi a7, zero, 64;
            ecall;
            // Exits with the number of bytes written, which the system call returned.
            addi a7, zero, 93;
            ecall;
            // The exit ended the first run. This is where the second run begins.
            addi a7, zero, 1;
            ecall;
        };
        let mut bus = SparseMemory::new();
        bus.write(0x1000, &code).unwrap();
        bus.write(0x700, &message).unwrap();
        let mut emu = Emulator::new(bus, MinimalKernel, Vec::new()).with_entry(0x1000);

        assert_eq!(emu.run(), Ok(300));
        assert_eq!(emu.ctx, message);

        emu.cpu.pc += 4;
        assert_eq!(emu.run(), Err(EmulationError::Kernel(UnknownSyscall(1))));
    }
}

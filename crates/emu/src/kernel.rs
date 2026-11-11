//! The interface to the environment that services a guest's system calls.
//!
//! The processor never calls a kernel. [Cpu::run] returns [Stop::Ecall] when the guest makes a
//! system call, and whoever drives the processor hands the guest's registers and memory to a
//! [Kernel] or an [AsyncKernel] before running it again.
//!
//! [Cpu::run]: brisc_hw::cpu::Cpu::run
//! [Stop::Ecall]: brisc_hw::cpu::Stop::Ecall

use brisc_hw::{cpu::Registers, memory::Memory, XWord};
use core::future::{ready, Future};

/// What the guest does after a system call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Syscall {
    /// The guest continues with the instruction after the `ecall`.
    Continue,
    /// The guest exits with this exit code.
    Exit(XWord),
}

/// A kernel that services system calls synchronously.
///
/// `S` is state that lives outside the kernel and is passed to every system call, for the kernel
/// to read and modify.
pub trait Kernel<S> {
    /// The error returned when a system call cannot be serviced.
    type Error;

    /// Services system call number `syscall_no`. Its arguments are in `registers` and `memory`,
    /// which also receive its results.
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        state: &mut S,
    ) -> Result<Syscall, Self::Error>;
}

/// A kernel that may suspend while it services a system call.
///
/// `S` is state that lives outside the kernel and is passed to every system call, for the kernel
/// to read and modify. Every [Kernel] is also an [AsyncKernel] that never suspends.
pub trait AsyncKernel<S> {
    /// The error returned when a system call cannot be serviced.
    type Error;

    /// Services system call number `syscall_no`. Its arguments are in `registers` and `memory`,
    /// which also receive its results.
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        state: &mut S,
    ) -> impl Future<Output = Result<Syscall, Self::Error>>;
}

impl<S, K: Kernel<S>> AsyncKernel<S> for K {
    type Error = K::Error;

    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        state: &mut S,
    ) -> impl Future<Output = Result<Syscall, Self::Error>> {
        ready(Kernel::syscall(self, syscall_no, memory, registers, state))
    }
}

/// The kernel of a guest that makes no system calls: every system call is an error.
impl<S> Kernel<S> for () {
    type Error = ();

    fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut Registers,
        _: &mut S,
    ) -> Result<Syscall, Self::Error> {
        Err(())
    }
}

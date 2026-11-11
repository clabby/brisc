//! The interface to the environment that services a guest's system calls.
//!
//! The processor never calls a kernel. [Cpu::run] returns [Stop::Ecall] when the guest makes a
//! system call, and whoever drives the processor hands the guest's registers and its bus to a
//! [Kernel] or an [AsyncKernel] before running it again.
//!
//! [Cpu::run]: brisc_hw::cpu::Cpu::run
//! [Stop::Ecall]: brisc_hw::cpu::Stop::Ecall

use brisc_hw::{bus::Bus, cpu::Registers, XWord};
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

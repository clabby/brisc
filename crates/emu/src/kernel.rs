//! The interface to the environment that services a guest's system calls.
//!
//! The processor never calls a kernel. [`Cpu::run`] returns [`Stop::Ecall`] when the guest makes a
//! system call, and whoever drives the processor hands the guest's registers and its memory to a
//! [`Kernel`] or an [`AsyncKernel`] before running it again.
//!
//! [`Cpu::run`]: brisc_hw::cpu::Cpu::run
//! [`Stop::Ecall`]: brisc_hw::cpu::Stop::Ecall

use crate::Budget;
use brisc_hw::{
    cpu::Registers,
    memory::{Address, Memory},
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
/// `K` is the error of the kernel and `M` the error of the memory. A kernel can apply `?` to the
/// result of an access to the memory, because an error of the memory converts to
/// [`SyscallError::Memory`].
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscallError<K, M> {
    /// The kernel cannot service the system call.
    #[error("the kernel cannot service the system call")]
    Kernel(K),
    /// The budget does not cover the system call: see [`Budget::charge`]. The system call has
    /// not been serviced, and running the guest again makes it again, after executing the `ecall`
    /// once more.
    #[error("the budget does not cover the system call")]
    Budget,
    /// The memory failed an access that the kernel made.
    #[error("the memory failed an access by the kernel")]
    Memory(M),
}

impl<K, M> From<M> for SyscallError<K, M> {
    fn from(error: M) -> Self {
        Self::Memory(error)
    }
}

/// A kernel that services system calls synchronously.
///
/// `S` is state that lives outside the kernel and is passed to every system call, for the kernel
/// to read and modify. `T` is the [`Tracer`](brisc_hw::cpu::Tracer) of the emulator that runs the
/// guest, which every system call receives so that the kernel can log what it does. A kernel that
/// ignores the tracer implements the trait for every `T`; one that logs asks for the tracer it
/// logs to. `budget` is what the guest may still spend, which the kernel can charge for its own
/// work. The tracer below never sees an instruction, because it does not ask for them, and still
/// records every system call, which costs ten units:
///
/// ```
/// use brisc_emu::{
///     kernel::{Kernel, Syscall, SyscallError},
///     Budget, EmulationError, Emulator,
/// };
/// use brisc_hw::{
///     cpu::{Registers, Step, Tracer},
///     memory::{Memory, SparseMemory},
///     rv_code, XWord,
/// };
///
/// /// What a kernel can log to.
/// trait Log {
///     fn log(&mut self, event: XWord);
/// }
///
/// #[derive(Default)]
/// struct Events(Vec<XWord>);
///
/// impl Log for Events {
///     fn log(&mut self, event: XWord) {
///         self.0.push(event);
///     }
/// }
///
/// impl<M: Memory, C> Tracer<M, C> for Events {
///     const ENABLED: bool = false;
///
///     fn trace(&mut self, _: &mut C, _: &M, _: &Registers, _: &Step) {}
/// }
///
/// struct LoggingKernel;
///
/// impl<T: Log> Kernel<(), T> for LoggingKernel {
///     type Error = ();
///
///     fn syscall<M: Memory>(
///         &mut self,
///         syscall_no: XWord,
///         _: &mut M,
///         _: &mut Registers,
///         _: &mut (),
///         tracer: &mut T,
///         budget: &mut Budget,
///     ) -> Result<Syscall, SyscallError<(), M::Error>> {
///         budget.charge(10)?;
///         tracer.log(syscall_no);
///         Ok(if syscall_no == 93 { Syscall::Exit(0) } else { Syscall::Continue })
///     }
/// }
///
/// let mut memory = SparseMemory::new();
/// let code = rv_code! {
///     addi a7, zero, 7;
///     ecall;
///     addi a7, zero, 93;
///     ecall;
/// };
/// memory.write(0x1000, &code).unwrap();
/// let mut emu =
///     Emulator::new(memory, LoggingKernel, ()).with_entry(0x1000).with_tracer(Events::default());
///
/// assert_eq!(emu.run(), Ok(0));
/// assert_eq!(emu.tracer().0, [7, 93]);
/// // The two system calls cost twenty units, and the four instructions four.
/// let parts = emu.into_parts();
/// assert_eq!(parts.budget.spent(), 24);
/// assert_eq!(parts.budget.remaining(), Budget::MAX - 24);
/// assert_eq!(parts.tracer.0, [7, 93]);
///
/// // A budget that covers the instructions but not the second system call stops it, with the
/// // first one logged.
/// let mut memory = SparseMemory::new();
/// memory.write(0x1000, &code).unwrap();
/// let mut emu = Emulator::new(memory, LoggingKernel, ())
///     .with_entry(0x1000)
///     .with_tracer(Events::default())
///     .with_budget(15);
/// assert_eq!(emu.run(), Err(EmulationError::Budget));
/// assert_eq!(emu.tracer().0, [7]);
/// ```
pub trait Kernel<S, T = ()> {
    /// The error returned when a system call cannot be serviced.
    type Error;

    /// Services system call number `syscall_no`. Its arguments are in `registers` and on the
    /// `memory`, which also receive its results.
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        state: &mut S,
        tracer: &mut T,
        budget: &mut Budget,
    ) -> Result<Syscall, SyscallError<Self::Error, M::Error>>;
}

/// A kernel that may suspend while it services a system call.
///
/// `S` is state that lives outside the kernel and is passed to every system call, for the kernel
/// to read and modify, and `T` is the tracer, as for [`Kernel`]. Every [`Kernel`] is also an
/// [`AsyncKernel`] that never suspends.
pub trait AsyncKernel<S, T = ()> {
    /// The error returned when a system call cannot be serviced.
    type Error;

    /// Services system call number `syscall_no`. Its arguments are in `registers` and on the
    /// `memory`, which also receive its results.
    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        state: &mut S,
        tracer: &mut T,
        budget: &mut Budget,
    ) -> impl Future<Output = Result<Syscall, SyscallError<Self::Error, M::Error>>>;
}

impl<S, T, K: Kernel<S, T>> AsyncKernel<S, T> for K {
    type Error = K::Error;

    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        state: &mut S,
        tracer: &mut T,
        budget: &mut Budget,
    ) -> impl Future<Output = Result<Syscall, SyscallError<Self::Error, M::Error>>> {
        ready(Kernel::syscall(self, syscall_no, memory, registers, state, tracer, budget))
    }
}

/// The kernel of a guest that makes no system calls: every system call is an error.
impl<S, T> Kernel<S, T> for () {
    type Error = ();

    fn syscall<M: Memory>(
        &mut self,
        _: XWord,
        _: &mut M,
        _: &mut Registers,
        _: &mut S,
        _: &mut T,
        _: &mut Budget,
    ) -> Result<Syscall, SyscallError<Self::Error, M::Error>> {
        Err(SyscallError::Kernel(()))
    }
}

/// A kernel with the two system calls that a program without an operating system needs, under
/// the numbers that Linux gives them on RISC-V.
///
/// - [`MinimalKernel::EXIT`] ends the program with the exit code in `a0`.
/// - [`MinimalKernel::WRITE`] appends the `a2` bytes at the address in `a1` to the state of the
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

impl<S: Extend<u8>, T> Kernel<S, T> for MinimalKernel {
    type Error = UnknownSyscall;

    fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        output: &mut S,
        _: &mut T,
        _: &mut Budget,
    ) -> Result<Syscall, SyscallError<Self::Error, M::Error>> {
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
                    memory.read(address, piece)?;
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
        let mut memory = SparseMemory::new();
        memory.write(0x1000, &code).unwrap();
        memory.write(0x700, &message).unwrap();
        let mut emu = Emulator::new(memory, MinimalKernel, Vec::new()).with_entry(0x1000);

        assert_eq!(emu.run(), Ok(300));
        assert_eq!(emu.ctx(), &message);

        emu.cpu_mut().pc += 4;
        assert_eq!(emu.run(), Err(EmulationError::Kernel(UnknownSyscall(1))));
    }
}

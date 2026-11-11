//! Single-threaded emulator.

use crate::cfg::EmuConfig;
use brisc_hw::{
    cpu::{Cpu, Fault, Stop},
    kernel::{AsyncKernel, Kernel, Syscall},
    XWord, REG_A7,
};
use thiserror::Error;

mod builder;
pub use builder::StEmuBuilder;

/// An error that can occur during emulation.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationError<E> {
    /// The guest tried to execute an instruction that cannot be executed.
    #[error(transparent)]
    Fault(#[from] Fault),

    /// An error that occurred in the kernel.
    #[error("the kernel failed to service a system call")]
    Kernel(E),
}

/// The error type of the kernel of `Config`.
pub type KernelError<'a, Config> =
    <<Config as EmuConfig<'a>>::Kernel as AsyncKernel<<Config as EmuConfig<'a>>::Context>>::Error;

/// A [`Result`] type alias for emulation results.
pub type EmulationResult<'a, T, Config> = Result<T, EmulationError<KernelError<'a, Config>>>;

/// A single RISC-V hart with its memory, the kernel that services its system calls, and the
/// context handed to the kernel.
///
/// [StEmu::run] drives a [Kernel] and [StEmu::run_async] an [AsyncKernel]. Both execute guest
/// instructions with the same [Cpu::run] and differ only in how they wait for the kernel.
#[derive(Debug)]
pub struct StEmu<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
{
    /// The processor.
    pub cpu: Cpu,
    /// The device memory.
    pub memory: Config::Memory,
    /// The system call interface.
    pub kernel: Config::Kernel,
    /// The emulator's context.
    pub ctx: Config::Context,
}

impl<'ctx, Config> StEmu<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
{
    /// Creates a new [`StEmuBuilder`].
    pub fn builder() -> StEmuBuilder<'ctx, Config> {
        StEmuBuilder::default()
    }

    /// Executes the program until it exits, returning its exit code.
    pub fn run(&mut self) -> EmulationResult<'ctx, XWord, Config>
    where
        Config::Kernel: Kernel<Config::Context, Error = KernelError<'ctx, Config>>,
    {
        loop {
            let syscall_no = self.run_to_syscall()?;
            let syscall = Kernel::syscall(
                &mut self.kernel,
                syscall_no,
                &mut self.memory,
                &mut self.cpu.registers,
                &mut self.ctx,
            )
            .map_err(EmulationError::Kernel)?;
            if let Some(exit_code) = self.finish_syscall(syscall) {
                return Ok(exit_code);
            }
        }
    }

    /// Executes the program until it exits, returning its exit code. Suspends whenever the
    /// kernel does.
    pub async fn run_async(&mut self) -> EmulationResult<'ctx, XWord, Config> {
        loop {
            let syscall_no = self.run_to_syscall()?;
            let syscall = AsyncKernel::syscall(
                &mut self.kernel,
                syscall_no,
                &mut self.memory,
                &mut self.cpu.registers,
                &mut self.ctx,
            )
            .await
            .map_err(EmulationError::Kernel)?;
            if let Some(exit_code) = self.finish_syscall(syscall) {
                return Ok(exit_code);
            }
        }
    }

    /// Executes instructions up to the next system call and returns its number.
    fn run_to_syscall(&mut self) -> Result<XWord, Fault> {
        loop {
            match self.cpu.run(&mut self.memory, u64::MAX) {
                Stop::Ecall => return Ok(self.cpu.registers.get(REG_A7)),
                Stop::Budget => {}
                Stop::Fault(fault) => return Err(fault),
            }
        }
    }

    /// Returns the exit code if the system call ended the program, and otherwise moves the
    /// program counter past the `ecall`.
    const fn finish_syscall(&mut self, syscall: Syscall) -> Option<XWord> {
        match syscall {
            Syscall::Continue => {
                self.cpu.pc = self.cpu.pc.wrapping_add(4);
                None
            }
            Syscall::Exit(exit_code) => Some(exit_code),
        }
    }

    /// Destroys the emulator and returns the context.
    pub fn take_ctx(self) -> Config::Context {
        self.ctx
    }
}

#[cfg(test)]
mod test {
    use crate::test_suites;

    test_suites!(
        base_dir = "../../rv-tests/bin",
        rv32ui ~ glob = "rv32ui-p-*",
        rv32um ~ glob = "rv32um-p-*" ~ must_have = ["m"],
        rv32ua ~ glob = "rv32ua-p-*" ~ must_have = ["a"],
        rv32uc ~ glob = "rv32uc-p-*" ~ must_have = ["c"],
        rv64ui ~ glob = "rv64ui-p-*" ~ must_have = ["64-bit"],
        rv64um ~ glob = "rv64um-p-*" ~ must_have = ["64-bit", "m"],
        rv64ua ~ glob = "rv64ua-p-*" ~ must_have = ["64-bit", "a"],
        rv64uc ~ glob = "rv64uc-p-*" ~ must_have = ["64-bit", "c"],
        rs_program_32 ~ glob = "rs-32bit-*" ~ must_have = ["m", "a", "c"] ~ must_not_have = ["64-bit"],
        rs_program_64 ~ glob = "rs-64bit-*" ~ must_have = ["64-bit", "m", "a", "c"]
    );
}

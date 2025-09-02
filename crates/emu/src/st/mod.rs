//! Single-threaded, single-cycle 5-stage RISC-V pipeline.

use crate::cfg::EmuConfig;
use brisc_hw::{
    errors::{PipelineError, PipelineResult},
    kernel::Kernel,
    pipeline::{
        decode_instruction, execute, instruction_fetch, mem_access, writeback, PipelineRegister,
    },
};

mod builder;
pub use builder::StEmuBuilder;

/// Single-cycle RISC-V processor emulator.
#[derive(Debug, Default)]
pub struct StEmu<'ctx, Config>
where
    Config: EmuConfig<'ctx>,
{
    /// The pipeline register.
    pub register: PipelineRegister,
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

    /// Executes the program until it exits, returning the final [PipelineRegister].
    #[cfg(not(feature = "async-kernel"))]
    pub fn run(&mut self) -> PipelineResult<PipelineRegister> {
        while !self.register.exit {
            self.cycle()?;
        }

        Ok(self.register)
    }

    /// Execute a single cycle of the processor in full.
    #[inline(always)]
    #[cfg(not(feature = "async-kernel"))]
    pub fn cycle(&mut self) -> PipelineResult<()> {
        let r = &mut self.register;

        // Execute all pipeline stages sequentially.
        let cycle_res = instruction_fetch(r, &self.memory)
            .and_then(|_| decode_instruction(r))
            .and_then(|_| execute(r))
            .and_then(|_| mem_access(r, &mut self.memory))
            .and_then(|_| writeback(r));

        // Handle system calls.
        match cycle_res {
            Ok(()) => {}
            Err(PipelineError::SyscallException(syscall_no)) => {
                self.kernel.syscall(syscall_no, &mut self.memory, r, &mut self.ctx)?;

                // Exit emulation if the syscall terminated the program.
                if r.exit {
                    return Ok(());
                }
            }
            Err(e) => return Err(e),
        }

        r.advance();
        Ok(())
    }

    /// Executes the program until it exits, returning the final [PipelineRegister].
    #[cfg(feature = "async-kernel")]
    pub async fn run(&mut self) -> PipelineResult<PipelineRegister> {
        while !self.register.exit {
            self.cycle().await?;
        }

        Ok(self.register)
    }

    /// Execute a single cycle of the processor in full.
    #[inline(always)]
    #[cfg(feature = "async-kernel")]
    pub async fn cycle(&mut self) -> PipelineResult<()> {
        let r = &mut self.register;

        // Execute all pipeline stages sequentially.
        let cycle_res = instruction_fetch(r, &self.memory)
            .and_then(|_| decode_instruction(r))
            .and_then(|_| execute(r))
            .and_then(|_| mem_access(r, &mut self.memory))
            .and_then(|_| writeback(r));

        // Handle system calls.
        match cycle_res {
            Ok(()) => {}
            Err(PipelineError::SyscallException(syscall_no)) => {
                self.kernel.syscall(syscall_no, &mut self.memory, r, &mut self.ctx).await?;

                // Exit emulation if the syscall terminated the program.
                if r.exit {
                    return Ok(());
                }
            }
            Err(e) => return Err(e),
        }

        r.advance();
        Ok(())
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

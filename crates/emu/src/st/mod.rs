//! Single-threaded, single-cycle 5-stage RISC-V pipeline.

use crate::cfg::EmuConfig;
use brisc_hw::{
    errors::{PipelineError, PipelineResult},
    linux::SyscallInterface,
    pipeline::{
        decode_instruction, execute, instruction_fetch, mem_access, writeback, PipelineRegister,
    },
    XWord,
};

/// Single-cycle RISC-V processor emulator.
#[derive(Debug, Default)]
pub struct StEmu<Config>
where
    Config: EmuConfig,
{
    /// The pipeline register.
    pub register: PipelineRegister,
    /// The device memory.
    pub memory: Config::Memory,
    /// The system call interface.
    pub syscall_interface: Config::SyscallInterface,
}

impl<Config> StEmu<Config>
where
    Config: EmuConfig,
{
    /// Create a new [StEmu] with the given [Memory] and [SyscallInterface].
    ///
    /// [Memory]: brisc_hw::memory::Memory
    pub fn new(
        pc: XWord,
        memory: Config::Memory,
        syscall_interface: Config::SyscallInterface,
    ) -> Self {
        Self { register: PipelineRegister::new(pc), memory, syscall_interface }
    }

    /// Executes the program until it exits, returning the final [PipelineRegister].
    pub fn run(&mut self) -> PipelineResult<PipelineRegister> {
        while !self.register.exit {
            self.cycle()?;
        }

        Ok(self.register)
    }

    /// Execute a single cycle of the processor in full.
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
            Err(PipelineError::SyscallException(syscall_no)) => {
                self.syscall_interface.syscall(syscall_no, &mut self.memory, r)?;

                // Exit emulation if the syscall terminated the program.
                if r.exit {
                    return Ok(());
                }
            }
            Err(e) => return Err(e),
            _ => {}
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
        rs_program_32 ~ glob = "rs-32bit-*" ~ must_have = ["m", "a", "c"],
        rs_program_64 ~ glob = "rs-64bit-*" ~ must_have = ["64-bit", "m", "a", "c"]
    );
}

//! Single-threaded, single-cycle 5-stage RISC-V pipeline.

use brisc_hw::{
    errors::{PipelineError, PipelineResult},
    linux::SyscallInterface,
    memory::Memory,
    pipeline::{
        decode_instruction, execute, instruction_fetch, mem_access, writeback, PipelineRegister,
    },
    XWord,
};

/// Single-cycle RISC-V processor emulator.
#[derive(Debug, Default)]
pub struct StEmu<M, S = ()>
where
    M: Memory + Default,
    S: SyscallInterface + Default,
{
    /// The pipeline register.
    pub register: PipelineRegister,
    /// The device memory.
    pub memory: M,
    /// The system call interface.
    pub syscall_interface: S,
}

impl<M, S> StEmu<M, S>
where
    M: Memory + Clone + Default,
    S: SyscallInterface + Default,
{
    /// Create a new [StEmu] with the given [Memory].
    pub fn new(pc: XWord, memory: M, syscall_interface: S) -> Self {
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
    use crate::elf::load_elf;
    use brisc_hw::{
        errors::PipelineResult,
        linux::SyscallInterface,
        memory::{Memory, SimpleMemory},
        XWord, REG_A0,
    };
    use rstest::rstest;
    use std::{fs, path::PathBuf};

    /// Creates a set of Rust tests for the RISC-V test suites passed.
    macro_rules! test_suites {
        (
            base_dir = $base_dir:literal,
            $($name:ident ~ glob = $glob:literal$( ~ must_have = [$($feature:literal$(,)?)+])?$(,)?)+
        ) => {
            $(
                #[rstest]
                $(#[cfg(all($(feature = $feature,)+))])?
                fn $name(
                    #[base_dir = $base_dir]
                    #[files($glob)]
                    #[exclude("\\.dump$")]
                    path: PathBuf,
                ) {
                    run_riscv_test(&path);
                }
            )+
        }
    }

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

    /// Helper function to run a single test case (RISCV standard test suite)
    fn run_riscv_test(test_path: &PathBuf) {
        // Load the program
        let elf_bytes = fs::read(test_path).unwrap();
        let mut hart = load_elf::<SimpleMemory, RiscvTestSyscalls>(&elf_bytes).unwrap();

        // Run the program until it exits
        let mut clock = 0;
        let now = std::time::Instant::now();
        while !hart.register.exit {
            hart.cycle().unwrap();
            clock += 1;
        }
        let elapsed = now.elapsed();

        println!("ips: {}", clock as f64 / elapsed.as_secs_f64());

        // Check the exit code
        assert_eq!(
            hart.register.exit_code,
            0,
            "Test failed: {:?} | Failing Test #: {} | clock: {clock}",
            test_path.file_name().unwrap(),
            hart.register.exit_code >> 1,
        );
    }

    #[derive(Default)]
    struct RiscvTestSyscalls;

    impl SyscallInterface for RiscvTestSyscalls {
        fn syscall<M: Memory>(
            &mut self,
            sysno: XWord,
            _: &mut M,
            p_reg: &mut brisc_hw::pipeline::PipelineRegister,
        ) -> PipelineResult<XWord> {
            match sysno {
                0x5D => {
                    let exit_code = p_reg.registers[REG_A0 as usize];
                    p_reg.exit_code = exit_code;
                    p_reg.exit = true;
                }
                _ => unimplemented!(),
            }

            Ok(0)
        }
    }
}

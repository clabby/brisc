//! Test utilities for the emulator crate.

use crate::{cfg::EmuConfig, st::StEmu};
use brisc_hw::{
    kernel::Kernel,
    memory::{Memory, SimpleMemory},
    pipeline::PipelineRegister,
    XWord, REG_A0, REG_A1, REG_A2,
};
use std::{fs, path::PathBuf};

use rstest as _;
use tracing_subscriber as _;

/// Creates a set of Rust tests for the RISC-V test suites passed.
#[macro_export]
macro_rules! test_suites {
    (
        base_dir = $base_dir:literal,
        $($name:ident ~ glob = $glob:literal$( ~ must_have = [$($feature:literal$(,)?)+])?$( ~ must_not_have = [$($no_feature:literal$(,)?)+])?$(,)?)+
    ) => {
        $(
            #[rstest::rstest]
            $(#[cfg(all($(feature = $feature,)+))])?
            $(#[cfg(all(not($(feature = $no_feature,)+)))])?
            fn $name(
                #[base_dir = $base_dir]
                #[files($glob)]
                #[exclude("\\.dump$")]
                path: std::path::PathBuf,
            ) {
                // Init global tracing subscriber
                tracing_subscriber::fmt()
                    .with_max_level(tracing::Level::DEBUG)
                    .init();
                tracing::info!(target: "test-runner", "Running test: {:?}", path.file_name().unwrap());

                $crate::test_utils::run_riscv_test(&path);
            }
        )+
    }
}

/// A RISC-V test program loaded into an emulator.
#[derive(Debug)]
pub struct TestProgram(StEmu<'static, TestStEmuConfig>);

impl TestProgram {
    /// Loads the ELF file `elf` into a fresh emulator.
    pub fn load(elf: &[u8]) -> Self {
        Self(
            StEmu::<TestStEmuConfig>::builder()
                .with_kernel(RiscvTestKernel)
                .with_ctx(())
                .with_elf(elf)
                .unwrap()
                .build(),
        )
    }

    /// Runs the program until it exits and returns the number of instructions it retired.
    ///
    /// ## Panics
    ///
    /// Panics if the program exits with a non-zero exit code.
    pub fn run(mut self) -> u64 {
        let mut instructions = 0;
        while !self.0.register.exit {
            self.0.cycle().unwrap();
            instructions += 1;
        }

        let exit_code = self.0.register.exit_code;
        assert_eq!(
            exit_code,
            0,
            "Test failed | Failing Test #: {} | instructions: {instructions}",
            exit_code >> 1,
        );
        instructions
    }
}

/// Runs the test program at `test_path` and returns the number of instructions it retired.
pub fn run_riscv_test(test_path: &PathBuf) -> u64 {
    TestProgram::load(&fs::read(test_path).unwrap()).run()
}

#[derive(Default, Debug)]
struct TestStEmuConfig;

impl EmuConfig<'_> for TestStEmuConfig {
    type Memory = SimpleMemory;

    type Kernel = RiscvTestKernel;

    type Context = ();
}

#[derive(Default, Debug)]
struct RiscvTestKernel;

impl Kernel<()> for RiscvTestKernel {
    type Error = ();

    fn syscall<M: Memory>(
        &mut self,
        sysno: XWord,
        mem: &mut M,
        p_reg: &mut PipelineRegister,
        _: &mut (),
    ) -> Result<XWord, Self::Error> {
        match sysno {
            0x5D => {
                let exit_code = p_reg.registers[REG_A0];
                p_reg.exit_code = exit_code;
                p_reg.exit = true;
            }
            0x40 => {
                let fd = p_reg.registers[REG_A0];
                let ptr = p_reg.registers[REG_A1];
                let len = p_reg.registers[REG_A2];

                let raw_msg = mem.read_memory_range(ptr, len).unwrap();
                let msg = String::from_utf8_lossy(&raw_msg);

                if fd == 1 {
                    tracing::info!(target: "test-runner", "stdout: {}", msg);
                } else if fd == 2 {
                    tracing::error!(target: "test-runner", "stderr: {}", msg);
                }
            }
            _ => unimplemented!(),
        }

        Ok(0)
    }
}

//! Test utilities for the emulator crate.

use crate::{cfg::EmuConfig, st::StEmu};
use brisc_hw::{
    errors::PipelineResult,
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

/// Helper function to run a single test case
pub fn run_riscv_test(test_path: &PathBuf) -> f64 {
    // Load the program
    let elf_bytes = fs::read(test_path).unwrap();
    let mut hart = StEmu::<TestStEmuConfig>::builder()
        .with_kernel(RiscvTestKernel)
        .with_elf(&elf_bytes)
        .unwrap()
        .build();

    // Run the program until it exits
    let mut clock = 0;
    let now = std::time::Instant::now();
    while !hart.register.exit {
        hart.cycle().unwrap();
        clock += 1;
    }

    let ips = clock as f64 / now.elapsed().as_secs_f64();
    tracing::info!(target: "test-runner", "ips: {ips}");

    // Check the exit code
    assert_eq!(
        hart.register.exit_code,
        0,
        "Test failed: {:?} | Failing Test #: {} | clock: {clock}",
        test_path.file_name().unwrap(),
        hart.register.exit_code >> 1,
    );

    ips
}

#[derive(Default)]
struct TestStEmuConfig;

impl EmuConfig for TestStEmuConfig {
    type Memory = SimpleMemory;

    type Kernel = RiscvTestKernel;
}

#[derive(Default)]
struct RiscvTestKernel;

impl Kernel for RiscvTestKernel {
    fn syscall<M: Memory>(
        &mut self,
        sysno: XWord,
        mem: &mut M,
        p_reg: &mut PipelineRegister,
    ) -> PipelineResult<XWord> {
        match sysno {
            0x5D => {
                let exit_code = p_reg.registers[REG_A0 as usize];
                p_reg.exit_code = exit_code;
                p_reg.exit = true;
            }
            0x40 => {
                let fd = p_reg.registers[REG_A0 as usize];
                let ptr = p_reg.registers[REG_A1 as usize];
                let len = p_reg.registers[REG_A2 as usize];

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

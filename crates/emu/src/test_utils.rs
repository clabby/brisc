//! Test utilities for the emulator crate.

use crate::{cfg::EmuConfig, elf::load_elf};
use brisc_hw::{
    errors::PipelineResult,
    linux::SyscallInterface,
    memory::{Memory, SimpleMemory},
    XWord, REG_A0,
};
use rstest as _;
use std::{fs, path::PathBuf};

/// Creates a set of Rust tests for the RISC-V test suites passed.
#[macro_export]
macro_rules! test_suites {
    (
        base_dir = $base_dir:literal,
        $($name:ident ~ glob = $glob:literal$( ~ must_have = [$($feature:literal$(,)?)+])?$(,)?)+
    ) => {
        $(
            #[rstest::rstest]
            $(#[cfg(all($(feature = $feature,)+))])?
            fn $name(
                #[base_dir = $base_dir]
                #[files($glob)]
                #[exclude("\\.dump$")]
                path: std::path::PathBuf,
            ) {
                $crate::test_utils::run_riscv_test(&path);
            }
        )+
    }
}

/// Helper function to run a single test case
pub fn run_riscv_test(test_path: &PathBuf) {
    // Load the program
    let elf_bytes = fs::read(test_path).unwrap();
    let mut hart = load_elf::<TestStEmuConfig>(&elf_bytes).unwrap();

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
struct TestStEmuConfig;

impl EmuConfig for TestStEmuConfig {
    type Memory = SimpleMemory;

    type SyscallInterface = RiscvTestSyscalls;

    type ExternalConfig = ();
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

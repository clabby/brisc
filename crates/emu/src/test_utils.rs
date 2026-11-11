//! Test utilities for the emulator crate.

use crate::{
    cfg::EmuConfig,
    kernel::{AsyncKernel, Kernel, Syscall},
    st::StEmu,
};
use brisc_hw::{
    cpu::{Registers, Stop},
    memory::{Memory, SparseMemory},
    XWord, REG_A0, REG_A1, REG_A2, REG_A7,
};
use std::{
    fs,
    future::Future,
    path::PathBuf,
    pin::{pin, Pin},
    task::{Context, Poll, Waker},
};

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
                .with_ctx(0)
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
        let exit_code = self.0.run().unwrap();
        let instructions = self.0.cpu.instret;
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
///
/// The program is run with a synchronous kernel, with an asynchronous kernel that suspends on
/// every system call, and in slices of a few instruction budgets. All runs must retire the same
/// instructions.
pub fn run_riscv_test(test_path: &PathBuf) -> u64 {
    let elf = fs::read(test_path).unwrap();
    let instructions = TestProgram::load(&elf).run();

    let mut emu = StEmu::<SuspendingConfig>::builder()
        .with_kernel(SuspendingKernel(RiscvTestKernel))
        .with_ctx(0)
        .with_elf(&elf)
        .unwrap()
        .build();
    assert_eq!(block_on(emu.run_async()), Ok(0));
    assert_eq!(emu.cpu.instret, instructions);
    // Every program exits through a system call, which the kernel counted in the context.
    assert!(emu.take_ctx() > 0);

    // The budgets surround the number of instructions the interpreter can run between checks of
    // its budget, which is what separates its exact and its fast mode.
    for budget in [1, 3, 2047, 2049] {
        assert_eq!(run_in_slices(&elf, budget), instructions, "budget: {budget}");
    }

    instructions
}

/// Runs the program with the processor limited to `budget` instructions at a time and returns the
/// number of instructions retired.
///
/// ## Panics
///
/// Panics if the program does not exit with code zero or if a slice retires the wrong number of
/// instructions.
fn run_in_slices(elf: &[u8], budget: u64) -> u64 {
    let mut emu = TestProgram::load(elf).0;
    loop {
        let retired = emu.cpu.instret;
        let stop = emu.cpu.run(&mut emu.memory, budget);
        match stop {
            Stop::Budget => assert_eq!(emu.cpu.instret - retired, budget),
            Stop::Ecall => {
                assert!(emu.cpu.instret - retired <= budget);
                let syscall = Kernel::syscall(
                    &mut emu.kernel,
                    emu.cpu.registers.get(REG_A7),
                    &mut emu.memory,
                    &mut emu.cpu.registers,
                    &mut emu.ctx,
                )
                .unwrap();
                match syscall {
                    Syscall::Continue => emu.cpu.pc = emu.cpu.pc.wrapping_add(4),
                    Syscall::Exit(code) => {
                        assert_eq!(code, 0);
                        return emu.cpu.instret;
                    }
                }
            }
            Stop::Fault(fault) => panic!("{fault}"),
        }
    }
}

#[derive(Default, Debug)]
struct TestStEmuConfig;

impl EmuConfig<'_> for TestStEmuConfig {
    type Memory = SparseMemory;

    type Kernel = RiscvTestKernel;

    /// The number of system calls serviced.
    type Context = u64;
}

#[derive(Default, Debug)]
struct SuspendingConfig;

impl EmuConfig<'_> for SuspendingConfig {
    type Memory = SparseMemory;

    type Kernel = SuspendingKernel;

    /// The number of system calls serviced.
    type Context = u64;
}

/// The kernel that the test programs expect: it implements `exit` and `write`.
#[derive(Default, Debug)]
struct RiscvTestKernel;

impl Kernel<u64> for RiscvTestKernel {
    /// The number of the unsupported system call.
    type Error = XWord;

    fn syscall<M: Memory>(
        &mut self,
        sysno: XWord,
        mem: &mut M,
        registers: &mut Registers,
        serviced: &mut u64,
    ) -> Result<Syscall, Self::Error> {
        *serviced += 1;
        match sysno {
            0x5D => Ok(Syscall::Exit(registers.get(REG_A0))),
            0x40 => {
                let fd = registers.get(REG_A0);
                let ptr = registers.get(REG_A1);
                let len = registers.get(REG_A2);

                let mut raw_msg = vec![0; len as usize];
                mem.read(ptr, &mut raw_msg);
                let msg = String::from_utf8_lossy(&raw_msg);

                if fd == 1 {
                    tracing::info!(target: "test-runner", "stdout: {}", msg);
                } else if fd == 2 {
                    tracing::error!(target: "test-runner", "stderr: {}", msg);
                }
                Ok(Syscall::Continue)
            }
            _ => Err(sysno),
        }
    }
}

/// A [RiscvTestKernel] that suspends once before it services each system call.
#[derive(Debug)]
struct SuspendingKernel(RiscvTestKernel);

impl AsyncKernel<u64> for SuspendingKernel {
    type Error = XWord;

    async fn syscall<M: Memory>(
        &mut self,
        sysno: XWord,
        mem: &mut M,
        registers: &mut Registers,
        serviced: &mut u64,
    ) -> Result<Syscall, Self::Error> {
        SuspendOnce(false).await;
        Kernel::syscall(&mut self.0, sysno, mem, registers, serviced)
    }
}

/// A future that is pending the first time it is polled.
struct SuspendOnce(bool);

impl Future for SuspendOnce {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<()> {
        if self.0 {
            Poll::Ready(())
        } else {
            self.0 = true;
            Poll::Pending
        }
    }
}

/// Polls `future` on the current thread until it completes.
fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
    }
}

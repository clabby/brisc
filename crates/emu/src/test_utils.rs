//! Test utilities for the emulator crate.

use crate::{
    kernel::{AsyncKernel, Kernel, MinimalKernel, Syscall, SyscallError, UnknownSyscall},
    EmulationError, Emulator,
};
use brisc_hw::{
    cpu::{Registers, Step, Stop, Tracer, Weights},
    memory::{Memory, SparseMemory},
    XWord, REG_A7,
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

/// The emulator that the test programs expect: plain memory, and a kernel that collects what the
/// program writes.
type TestEmulator<K> = Emulator<SparseMemory, K, Vec<u8>>;

/// Loads the ELF file elf into a new emulator.
fn load<K>(kernel: K, elf: &[u8]) -> TestEmulator<K> {
    let mut emu = Emulator::new(SparseMemory::new(), kernel, Vec::new());
    emu.load_elf(elf).unwrap();
    emu
}

/// Makes the JIT compiler, if there is one, compile regions at once and on the thread of the
/// emulator. A run then covers as much of the program as the JIT can, and does not depend on
/// timing.
#[cfg_attr(not(feature = "jit"), allow(clippy::missing_const_for_fn))]
fn deterministic<K>(emu: TestEmulator<K>) -> TestEmulator<K> {
    #[cfg(feature = "jit")]
    let emu = emu.with_jit(Some(brisc_hw::cpu::JitConfig {
        threshold: 1,
        compilation: brisc_hw::cpu::Compilation::Inline,
        max_region_instructions: 1500,
        optimize_after: Some(2_000),
    }));
    emu
}

/// A RISC-V test program loaded into an emulator with the default configuration.
#[derive(Debug)]
pub struct TestProgram(TestEmulator<MinimalKernel>);

impl TestProgram {
    /// Loads the ELF file elf into a fresh emulator.
    pub fn load(elf: &[u8]) -> Self {
        Self(load(MinimalKernel, elf))
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
            "Test failed | Failing Test #: {} | instructions: {instructions} | output: {}",
            exit_code >> 1,
            String::from_utf8_lossy(&self.0.ctx),
        );
        instructions
    }
}

/// Runs the test program at `test_path` and returns the number of instructions it retired.
///
/// The program is run with the default configuration of the emulator, with an asynchronous kernel
/// that suspends on every system call, and in slices of a few instruction budgets. All runs must
/// retire the same instructions. The last three runs are deterministic: see [`deterministic`].
pub fn run_riscv_test(test_path: &PathBuf) -> u64 {
    let elf = fs::read(test_path).unwrap();
    let instructions = TestProgram::load(&elf).run();

    let mut emu = deterministic(load(SuspendingKernel::default(), &elf));
    assert_eq!(block_on(emu.run_async()), Ok(0));
    assert_eq!(emu.cpu.instret, instructions);
    // Every program exits through a system call.
    assert!(emu.kernel.suspensions > 0);
    tracing::info!(target: "test-runner", "output: {}", String::from_utf8_lossy(&emu.take_ctx()));

    // A tracer sees each retired instruction once and in order, and does not change the run.
    let mut emu = load(MinimalKernel, &elf).with_tracer(CountingTracer::default());
    assert_eq!(emu.run(), Ok(0));
    assert_eq!(emu.cpu.instret, instructions);
    // Every instruction is traced once, including each ecall.
    assert_eq!(emu.tracer.traced, instructions);

    check_budget(&elf, instructions);

    // The budgets surround the number of instructions the interpreter can run between checks of
    // its budget, which is what separates its exact and its fast mode.
    for budget in [1, 3, 2047, 2049] {
        assert_eq!(run_in_slices(&elf, budget), instructions, "budget: {budget}");
    }

    instructions
}

/// Gives every kind of instruction a different weight.
fn weights() -> Weights {
    Weights::new(|opcode| 1 + opcode as u32 % 5)
}

/// Runs the program with weighted instructions, on the interpreter and on compiled code, and
/// checks that both spend the same and that the budget is exact: the run completes with exactly
/// what it spends, and with one unit less it stops, and completes once the unit is added.
///
/// ## Panics
///
/// Panics if a run does not behave like that.
fn check_budget(elf: &[u8], instructions: u64) {
    let interpreted = load(MinimalKernel, elf).with_weights(weights());
    #[cfg(feature = "jit")]
    let interpreted = interpreted.with_jit(None);
    let mut interpreted = interpreted;
    assert_eq!(interpreted.run(), Ok(0));
    let spent = interpreted.cpu.spent;
    assert_eq!(interpreted.cpu.instret, instructions);
    assert!(spent >= instructions);

    let mut compiled = deterministic(load(MinimalKernel, elf).with_weights(weights()));
    assert_eq!(compiled.run(), Ok(0));
    assert_eq!((compiled.cpu.instret, compiled.cpu.spent), (instructions, spent));

    let mut exact =
        deterministic(load(MinimalKernel, elf).with_weights(weights()).with_budget(spent));
    assert_eq!(exact.run(), Ok(0));
    assert_eq!(exact.budget, 0);

    let mut short =
        deterministic(load(MinimalKernel, elf).with_weights(weights()).with_budget(spent - 1));
    assert_eq!(short.run(), Err(EmulationError::Budget));
    // The run stopped because the next instruction, which weighs at most five, did not fit.
    assert!(short.cpu.spent < spent && short.budget < 5);
    short.budget += spent - short.cpu.spent;
    assert_eq!(short.run(), Ok(0));
    assert_eq!((short.cpu.instret, short.cpu.spent), (instructions, spent));
}

/// Runs the program with the processor limited to `budget` instructions at a time and returns the
/// number of instructions retired.
///
/// ## Panics
///
/// Panics if the program does not exit with code zero or if a slice retires the wrong number of
/// instructions.
fn run_in_slices(elf: &[u8], budget: u64) -> u64 {
    let mut emu = deterministic(load(MinimalKernel, elf));
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
                    &mut (),
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
            Stop::Ebreak => panic!("ebreak"),
            Stop::Memory(error) => match error {},
        }
    }
}

/// A [`Tracer`] that checks the order of the instructions and counts them.
#[derive(Default, Debug)]
struct CountingTracer {
    /// The number of instructions traced.
    traced: u64,
}

impl Tracer<SparseMemory, Vec<u8>> for CountingTracer {
    fn trace(&mut self, _: &mut Vec<u8>, _: &SparseMemory, _: &Registers, step: &Step) {
        assert_eq!(step.instret, self.traced);
        self.traced += 1;
    }
}

/// A [`MinimalKernel`] that suspends once before it services each system call.
#[derive(Default, Debug)]
struct SuspendingKernel {
    /// The number of times the kernel has suspended.
    suspensions: u64,
}

impl AsyncKernel<Vec<u8>> for SuspendingKernel {
    type Error = UnknownSyscall;

    async fn syscall<M: Memory>(
        &mut self,
        syscall_no: XWord,
        memory: &mut M,
        registers: &mut Registers,
        output: &mut Vec<u8>,
        tracer: &mut (),
    ) -> Result<Syscall, SyscallError<UnknownSyscall, M::Error>> {
        SuspendOnce(false).await;
        self.suspensions += 1;
        Kernel::syscall(&mut MinimalKernel, syscall_no, memory, registers, output, tracer)
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

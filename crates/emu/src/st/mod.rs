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
    use super::*;
    use crate::test_suites;
    use brisc_hw::{
        memory::{Memory, SparseMemory},
        rv, rv_code, REG_A0,
    };

    /// A configuration whose guest makes no system calls other than the one that ends a test.
    struct NoKernel;

    impl EmuConfig<'_> for NoKernel {
        type Memory = SparseMemory;
        type Kernel = ();
        type Context = ();
    }

    #[test]
    fn executing_unmapped_memory_faults() {
        // Unmapped memory reads as zero, which is not an instruction.
        let mut emu = StEmu::<NoKernel>::builder()
            .with_memory(SparseMemory::new())
            .with_kernel(())
            .with_ctx(())
            .build();
        assert!(matches!(emu.run(), Err(EmulationError::Fault(Fault::Decode(_)))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (0, 0));
    }

    /// Builds an emulator that starts at `pc` in `memory`.
    fn emulator(pc: XWord, memory: SparseMemory) -> StEmu<'static, NoKernel> {
        StEmu::builder().with_pc(pc).with_memory(memory).with_kernel(()).with_ctx(()).build()
    }

    /// The address of an instruction whose first half is the end of a page and whose second
    /// half is the start of the next one.
    const STRADDLING: XWord = 0x1ffe;

    #[test]
    fn rewriting_the_second_half_of_an_instruction_on_the_next_page() {
        let mut memory = SparseMemory::new();
        memory.write(STRADDLING, &rv_code! { addi a0, zero, 5; ecall });
        let mut emu = emulator(STRADDLING, memory);

        // The kernel refuses the system call, which ends the run after the instruction.
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 5);

        // The second half holds the immediate.
        emu.memory.write(STRADDLING + 2, &rv!(addi a0, zero, 7).to_le_bytes()[2..]);
        emu.cpu.pc = STRADDLING;
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 7);
    }

    #[test]
    fn completing_an_illegal_instruction_on_the_next_page() {
        // The second half holds the shift amount and, above it, bits that must be zero.
        let slli = rv!(slli a0, a0, 1).to_le_bytes();
        let mut memory = SparseMemory::new();
        memory.write(STRADDLING, &[slli[0], slli[1], 0xff, 0xff]);
        memory.write(STRADDLING + 4, &rv!(ecall).to_le_bytes());
        let mut emu = emulator(STRADDLING, memory);
        emu.cpu.registers.set(REG_A0, 3);

        assert!(matches!(emu.run(), Err(EmulationError::Fault(Fault::Decode(_)))));
        assert_eq!(emu.cpu.pc, STRADDLING);

        emu.memory.write(STRADDLING + 2, &slli[2..]);
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 6);
    }

    #[test]
    #[cfg(all(feature = "64-bit", feature = "a"))]
    fn lr_w_sign_extends() {
        use brisc_hw::REG_A1;

        let mut memory = SparseMemory::new();
        memory.write(0x1000, &rv_code! { lr.w a0, (a1); ecall });
        memory.write(0x2000, &0x8000_0000u32.to_le_bytes());

        let mut emu = emulator(0x1000, memory);
        emu.cpu.registers.set(REG_A1, 0x2000);

        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 0xffff_ffff_8000_0000);
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
        rs_program_32 ~ glob = "rs-32bit-*" ~ must_have = ["m", "a", "c"] ~ must_not_have = ["64-bit"],
        rs_program_64 ~ glob = "rs-64bit-*" ~ must_have = ["64-bit", "m", "a", "c"]
    );
}

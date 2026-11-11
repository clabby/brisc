//! Single-threaded emulator.

use crate::{
    cfg::EmuConfig,
    kernel::{AsyncKernel, Kernel, Syscall, SyscallError},
};
use brisc_hw::{
    bus::Bus,
    cpu::{Cpu, Fault, Stop},
    XWord, REG_A7,
};
use thiserror::Error;

mod builder;
pub use builder::StEmuBuilder;

/// An error that can occur during emulation. `K` is the error of the kernel and `B` the error
/// of the bus.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationError<K, B> {
    /// The guest tried to execute an instruction that cannot be executed.
    #[error(transparent)]
    Fault(#[from] Fault),

    /// The guest executed an `ebreak`, which the program counter is left on. To continue,
    /// advance [Cpu::pc] past it as [Stop::Ebreak] describes.
    #[error("the guest executed an ebreak")]
    Ebreak,

    /// The bus failed an access, by the guest or by the kernel.
    #[error("the bus failed an access")]
    Bus(B),

    /// An error that occurred in the kernel.
    #[error("the kernel failed to service a system call")]
    Kernel(K),
}

impl<K, B> From<SyscallError<K, B>> for EmulationError<K, B> {
    fn from(error: SyscallError<K, B>) -> Self {
        match error {
            SyscallError::Kernel(error) => Self::Kernel(error),
            SyscallError::Bus(error) => Self::Bus(error),
        }
    }
}

/// The error type of the kernel of `Config`.
pub type KernelError<'a, Config> =
    <<Config as EmuConfig<'a>>::Kernel as AsyncKernel<<Config as EmuConfig<'a>>::Context>>::Error;

/// The error type of the bus of `Config`.
pub type BusError<'a, Config> = <<Config as EmuConfig<'a>>::Bus as Bus>::Error;

/// A [`Result`] type alias for emulation results.
pub type EmulationResult<'a, T, Config> =
    Result<T, EmulationError<KernelError<'a, Config>, BusError<'a, Config>>>;

/// A single RISC-V hart with its bus, the kernel that services its system calls, and the
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
    /// The memory and devices of the processor.
    pub bus: Config::Bus,
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
    ///
    /// After an error the emulator is as it was before the instruction or the system call that
    /// failed, and running it again tries that again.
    pub fn run(&mut self) -> EmulationResult<'ctx, XWord, Config>
    where
        Config::Kernel: Kernel<Config::Context, Error = KernelError<'ctx, Config>>,
    {
        loop {
            let syscall_no = self.run_to_syscall()?;
            let syscall = Kernel::syscall(
                &mut self.kernel,
                syscall_no,
                &mut self.bus,
                &mut self.cpu.registers,
                &mut self.ctx,
            )?;
            if let Some(exit_code) = self.finish_syscall(syscall) {
                return Ok(exit_code);
            }
        }
    }

    /// Executes the program until it exits, returning its exit code. Suspends whenever the
    /// kernel does.
    ///
    /// After an error the emulator is as it was before the instruction or the system call that
    /// failed, and running it again tries that again.
    pub async fn run_async(&mut self) -> EmulationResult<'ctx, XWord, Config> {
        loop {
            let syscall_no = self.run_to_syscall()?;
            let syscall = AsyncKernel::syscall(
                &mut self.kernel,
                syscall_no,
                &mut self.bus,
                &mut self.cpu.registers,
                &mut self.ctx,
            )
            .await?;
            if let Some(exit_code) = self.finish_syscall(syscall) {
                return Ok(exit_code);
            }
        }
    }

    /// Executes instructions up to the next system call and returns its number.
    fn run_to_syscall(&mut self) -> EmulationResult<'ctx, XWord, Config> {
        loop {
            match self.cpu.run(&mut self.bus, u64::MAX) {
                Stop::Ecall => return Ok(self.cpu.registers.get(REG_A7)),
                Stop::Ebreak => return Err(EmulationError::Ebreak),
                Stop::Budget => {}
                Stop::Fault(fault) => return Err(EmulationError::Fault(fault)),
                Stop::Bus(error) => return Err(EmulationError::Bus(error)),
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
        bus::{Address, PageIndex},
        cpu::Registers,
        memory::SparseMemory,
        rv, rv_code, REG_A0, REG_A1, REG_A2,
    };

    /// A guest in plain memory that makes no system calls other than the one that ends a test.
    struct NoKernel;

    impl EmuConfig<'_> for NoKernel {
        type Bus = SparseMemory;
        type Kernel = ();
        type Context = ();
    }

    /// Builds an emulator that starts at `pc` on `bus`.
    fn emulator<Config>(
        pc: XWord,
        bus: Config::Bus,
        kernel: Config::Kernel,
    ) -> StEmu<'static, Config>
    where
        Config: EmuConfig<'static, Context = ()>,
    {
        StEmu::builder().with_pc(pc).with_bus(bus).with_kernel(kernel).with_ctx(()).build()
    }

    #[test]
    fn executing_unmapped_memory_faults() {
        // Unmapped memory reads as zero, which is not an instruction.
        let mut emu = emulator::<NoKernel>(0, SparseMemory::new(), ());
        assert!(matches!(emu.run(), Err(EmulationError::Fault(Fault::Decode(_)))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (0, 0));
    }

    /// The address of an instruction whose first half is the end of a page and whose second
    /// half is the start of the next one.
    const STRADDLING: XWord = 0x1ffe;

    #[test]
    fn rewriting_the_second_half_of_an_instruction_on_the_next_page() {
        let mut memory = SparseMemory::new();
        memory.write(STRADDLING, &rv_code! { addi a0, zero, 5; ecall }).unwrap();
        let mut emu = emulator::<NoKernel>(STRADDLING, memory, ());

        // The kernel refuses the system call, which ends the run after the instruction.
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 5);

        // The second half holds the immediate.
        emu.bus.write(STRADDLING + 2, &rv!(addi a0, zero, 7).to_le_bytes()[2..]).unwrap();
        emu.cpu.pc = STRADDLING;
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 7);
    }

    #[test]
    fn completing_an_illegal_instruction_on_the_next_page() {
        // The second half holds the shift amount and, above it, bits that must be zero.
        let slli = rv!(slli a0, a0, 1).to_le_bytes();
        let mut memory = SparseMemory::new();
        memory.write(STRADDLING, &[slli[0], slli[1], 0xff, 0xff]).unwrap();
        memory.write(STRADDLING + 4, &rv!(ecall).to_le_bytes()).unwrap();
        let mut emu = emulator::<NoKernel>(STRADDLING, memory, ());
        emu.cpu.registers.set(REG_A0, 3);

        assert!(matches!(emu.run(), Err(EmulationError::Fault(Fault::Decode(_)))));
        assert_eq!(emu.cpu.pc, STRADDLING);

        emu.bus.write(STRADDLING + 2, &slli[2..]).unwrap();
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 6);
    }

    #[test]
    #[cfg(all(feature = "64-bit", feature = "a"))]
    fn lr_w_sign_extends() {
        let mut memory = SparseMemory::new();
        memory.write(0x1000, &rv_code! { lr.w a0, (a1); ecall }).unwrap();
        memory.write(0x2000, &0x8000_0000u32.to_le_bytes()).unwrap();

        let mut emu = emulator::<NoKernel>(0x1000, memory, ());
        emu.cpu.registers.set(REG_A1, 0x2000);

        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 0xffff_ffff_8000_0000);
    }

    #[test]
    fn ebreak_stops_before_it_is_retired() {
        let mut memory = SparseMemory::new();
        let code = rv_code! {
            addi a0, zero, 1;
            ebreak;
            addi a0, a0, 1;
            ecall;
        };
        memory.write(0x1000, &code).unwrap();
        let mut emu = emulator::<NoKernel>(0x1000, memory, ());

        assert_eq!(emu.run(), Err(EmulationError::Ebreak));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (0x1004, 1));

        emu.cpu.pc += 4;
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 2);
    }

    #[test]
    fn control_and_status_registers_read_as_zero() {
        const MSCRATCH: i64 = 0x340;
        const CYCLE: i64 = 0xc00;

        let mut memory = SparseMemory::new();
        let code = rv_code! {
            csrrw a0, {MSCRATCH}, a1;
            csrrs a1, {MSCRATCH}, zero;
            csrrs a2, {CYCLE}, zero;
            csrrw zero, {CYCLE}, zero;
        };
        memory.write(0x1000, &code).unwrap();
        let mut emu = emulator::<NoKernel>(0x1000, memory, ());
        for register in [REG_A0, REG_A1, REG_A2] {
            emu.cpu.registers.set(register, 5);
        }

        // The last instruction is what assemblers call `unimp`: a write to a read-only register.
        assert_eq!(emu.run(), Err(EmulationError::Fault(Fault::ReadOnlyCsr(0xc00))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (0x100c, 3));
        for register in [REG_A0, REG_A1, REG_A2] {
            assert_eq!(emu.cpu.registers.get(register), 0);
        }
    }

    /// A bus with memory below [BoundedMemory::END] and nothing above it. It implements no more
    /// of [Bus] than it has to.
    #[derive(Default)]
    struct BoundedMemory(SparseMemory);

    /// An access that reaches beyond the memory of a [BoundedMemory], with its address.
    #[derive(Debug, PartialEq, Eq)]
    struct OutOfBounds(Address);

    impl BoundedMemory {
        const END: Address = 0x4000;

        fn check(address: Address, len: usize) -> Result<(), OutOfBounds> {
            match address.checked_add(len as Address) {
                Some(end) if end <= Self::END => Ok(()),
                _ => Err(OutOfBounds(address)),
            }
        }
    }

    impl Bus for BoundedMemory {
        type Error = OutOfBounds;

        fn load<const N: usize>(&mut self, address: Address) -> Result<[u8; N], OutOfBounds> {
            Self::check(address, N)?;
            let Ok(bytes) = self.0.load(address);
            Ok(bytes)
        }

        fn store<const N: usize>(
            &mut self,
            address: Address,
            bytes: [u8; N],
        ) -> Result<bool, OutOfBounds> {
            Self::check(address, N)?;
            let Ok(watched) = self.0.store(address, bytes);
            Ok(watched)
        }

        fn watch(&mut self, page: PageIndex) {
            self.0.watch(page);
        }

        fn pop_dirty(&mut self) -> Option<PageIndex> {
            self.0.pop_dirty()
        }
    }

    /// A kernel whose only system call exits with the byte at the address in `a0`.
    struct LoadKernel;

    impl Kernel<()> for LoadKernel {
        type Error = ();

        fn syscall<B: Bus>(
            &mut self,
            _: XWord,
            bus: &mut B,
            registers: &mut Registers,
            _: &mut (),
        ) -> Result<Syscall, SyscallError<(), B::Error>> {
            let [byte] = bus.load(registers.get(REG_A0))?;
            Ok(Syscall::Exit(byte as XWord))
        }
    }

    struct Bounded;

    impl EmuConfig<'_> for Bounded {
        type Bus = BoundedMemory;
        type Kernel = LoadKernel;
        type Context = ();
    }

    #[test]
    fn a_store_that_the_bus_fails_can_be_tried_again() {
        let mut bus = BoundedMemory::default();
        let code = rv_code! {
            addi a0, zero, 7;
            sb a0, 0(a1);
            add a0, zero, a1;
            ecall;
        };
        bus.write(0x1000, &code).unwrap();
        let mut emu = emulator::<Bounded>(0x1000, bus, LoadKernel);
        emu.cpu.registers.set(REG_A1, BoundedMemory::END);

        // The store stops the run before it has changed anything.
        assert_eq!(emu.run(), Err(EmulationError::Bus(OutOfBounds(BoundedMemory::END))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (0x1004, 1));

        // With an address that exists, the run reaches the system call, which reads the byte.
        emu.cpu.registers.set(REG_A1, 0x2000);
        assert_eq!(emu.run(), Ok(7));
    }

    #[test]
    fn the_bus_can_fail_a_fetch_and_an_access_by_the_kernel() {
        // The last instruction that the memory has room for.
        let last = BoundedMemory::END - 4;
        let mut bus = BoundedMemory::default();
        bus.write(last, &rv!(addi a0, zero, 1).to_le_bytes()).unwrap();
        let mut emu = emulator::<Bounded>(last, bus, LoadKernel);

        // Fetching it does not read past the end, but there is no instruction after it.
        assert_eq!(emu.run(), Err(EmulationError::Bus(OutOfBounds(BoundedMemory::END))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (BoundedMemory::END, 1));

        // The kernel passes on the error of its own access.
        emu.bus.write(last, &rv!(ecall).to_le_bytes()).unwrap();
        emu.cpu.pc = last;
        emu.cpu.registers.set(REG_A0, BoundedMemory::END);
        assert_eq!(emu.run(), Err(EmulationError::Bus(OutOfBounds(BoundedMemory::END))));

        // The system call is made again by the next run, now with the address of the `ecall`.
        emu.cpu.registers.set(REG_A0, last);
        assert_eq!(emu.run(), Ok(rv!(ecall) as XWord));
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

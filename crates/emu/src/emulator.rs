//! The emulator: a hart with its memory, and the kernel that services its system calls.

use crate::{
    elf::{load_elf, LoadError},
    kernel::{AsyncKernel, Kernel, Syscall, SyscallError},
};
use brisc_hw::{
    cpu::{Cpu, Fault, Stop, Tracer},
    memory::Memory,
    XWord, REG_A7,
};
use thiserror::Error;

/// An error that can occur during emulation. K is the error of the kernel and M the error of the
/// memory.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmulationError<K, M> {
    /// The guest tried to execute an instruction that cannot be executed.
    #[error(transparent)]
    Fault(#[from] Fault),

    /// The guest executed an ebreak, which the program counter is left on. To continue, advance
    /// [`Cpu::pc`] past it as [`Stop::Ebreak`] describes.
    #[error("the guest executed an ebreak")]
    Ebreak,

    /// The memory failed an access, by the guest or by the kernel.
    #[error("the memory failed an access")]
    Memory(M),

    /// An error that occurred in the kernel.
    #[error("the kernel failed to service a system call")]
    Kernel(K),
}

impl<K, M> From<SyscallError<K, M>> for EmulationError<K, M> {
    fn from(error: SyscallError<K, M>) -> Self {
        match error {
            SyscallError::Kernel(error) => Self::Kernel(error),
            SyscallError::Memory(error) => Self::Memory(error),
        }
    }
}

/// A RISC-V hart with its memory, the kernel that services its system calls, and the state that
/// the kernel works on.
///
/// - M is the [`Memory`] that the hart runs on, which can include devices.
/// - K is the kernel. [`Emulator::run`] drives a [`Kernel`] and [`Emulator::run_async`] an
///   [`AsyncKernel`]. Both execute guest instructions with the same [`Cpu::run`] and differ only in
///   how they wait for the kernel.
/// - S is the state that every system call receives. It can be anything, including a borrow.
/// - T is the [`Tracer`] that sees every instruction before it executes, and is given the state S
///   to change. The default, `()`, does not trace and costs nothing. A tracer turns the JIT
///   compiler off, as [`Cpu::run_traced`] describes.
///
/// The hart is a public field, so its registers, program counter and configuration are there to
/// inspect and change between runs.
#[derive(Debug)]
pub struct Emulator<M, K, S = (), T = ()> {
    /// The hart.
    pub cpu: Cpu,
    /// The memory of the hart, with any devices.
    pub memory: M,
    /// The system call interface.
    pub kernel: K,
    /// The state that the kernel works on.
    pub ctx: S,
    /// The tracer of the instructions.
    pub tracer: T,
}

impl<M: Memory, K, S> Emulator<M, K, S> {
    /// Creates an emulator whose hart starts at address zero and does not trace.
    pub fn new(memory: M, kernel: K, ctx: S) -> Self {
        Self { cpu: Cpu::new(0), memory, kernel, ctx, tracer: () }
    }
}

impl<M: Memory, K, S, T: Tracer<M, S>> Emulator<M, K, S, T> {
    /// Sets the tracer that sees every instruction, in place of the one that there is.
    #[must_use]
    pub fn with_tracer<T2: Tracer<M, S>>(self, tracer: T2) -> Emulator<M, K, S, T2> {
        let Self { cpu, memory, kernel, ctx, .. } = self;
        Emulator { cpu, memory, kernel, ctx, tracer }
    }

    /// Sets the program counter, which is where the program starts.
    #[must_use]
    pub const fn with_entry(mut self, pc: XWord) -> Self {
        self.cpu.pc = pc;
        self
    }

    /// Sets how the hart uses the JIT compiler, or turns it off with None.
    #[cfg(feature = "jit")]
    #[must_use]
    pub fn with_jit(mut self, config: Option<brisc_hw::cpu::JitConfig>) -> Self {
        self.cpu.set_jit(config);
        self
    }

    /// Loads an ELF file onto the memory and sets the program counter to its entry point.
    pub fn load_elf(&mut self, elf: &[u8]) -> Result<(), LoadError<M::Error>> {
        self.cpu.pc = load_elf(&mut self.memory, elf)?;
        Ok(())
    }

    /// Destroys the emulator and returns the state that the kernel works on.
    pub fn take_ctx(self) -> S {
        self.ctx
    }

    /// Executes the program until it exits, returning its exit code.
    ///
    /// After a fault, an `ebreak` or an error of the memory, the emulator is as it was before the
    /// instruction that failed, and running it again tries that again. After an error of the
    /// kernel the `ecall` has been executed and counted, and running again makes the system call
    /// again; the kernel may have changed the registers, the memory and the state before it failed.
    pub fn run(&mut self) -> Result<XWord, EmulationError<K::Error, M::Error>>
    where
        K: Kernel<S>,
    {
        loop {
            let syscall_no = self.run_to_syscall()?;
            let syscall = Kernel::syscall(
                &mut self.kernel,
                syscall_no,
                &mut self.memory,
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
    /// Errors leave the emulator as they do for [`Emulator::run`].
    pub async fn run_async(&mut self) -> Result<XWord, EmulationError<K::Error, M::Error>>
    where
        K: AsyncKernel<S>,
    {
        loop {
            let syscall_no = self.run_to_syscall()?;
            let syscall = AsyncKernel::syscall(
                &mut self.kernel,
                syscall_no,
                &mut self.memory,
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
    fn run_to_syscall<E>(&mut self) -> Result<XWord, EmulationError<E, M::Error>> {
        loop {
            match self.cpu.run_traced(&mut self.memory, u64::MAX, &mut self.tracer, &mut self.ctx) {
                Stop::Ecall => return Ok(self.cpu.registers.get(REG_A7)),
                Stop::Ebreak => return Err(EmulationError::Ebreak),
                Stop::Budget => {}
                Stop::Fault(fault) => return Err(EmulationError::Fault(fault)),
                Stop::Memory(error) => return Err(EmulationError::Memory(error)),
            }
        }
    }

    /// Returns the exit code if the system call ended the program, and otherwise moves the
    /// program counter past the ecall.
    const fn finish_syscall(&mut self, syscall: Syscall) -> Option<XWord> {
        match syscall {
            Syscall::Continue => {
                self.cpu.pc = self.cpu.pc.wrapping_add(4);
                None
            }
            Syscall::Exit(exit_code) => Some(exit_code),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_suites;
    use brisc_hw::{
        cpu::Registers,
        memory::{Address, PageIndex, SparseMemory},
        rv, rv_code, REG_A0, REG_A1, REG_A2,
    };

    /// An emulator for a guest that has no kernel and no state, starting at pc.
    fn emulator<M: Memory, K>(pc: XWord, memory: M, kernel: K) -> Emulator<M, K> {
        Emulator::new(memory, kernel, ()).with_entry(pc)
    }

    #[test]
    fn executing_unmapped_memory_faults() {
        // Unmapped memory reads as zero, which is not an instruction.
        let mut emu = emulator(0, SparseMemory::new(), ());
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
        let mut emu = emulator(STRADDLING, memory, ());

        // The kernel refuses the system call, which ends the run after the instruction.
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 5);

        // The second half holds the immediate.
        emu.memory.write(STRADDLING + 2, &rv!(addi a0, zero, 7).to_le_bytes()[2..]).unwrap();
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
        let mut emu = emulator(STRADDLING, memory, ());
        emu.cpu.registers.set(REG_A0, 3);

        assert!(matches!(emu.run(), Err(EmulationError::Fault(Fault::Decode(_)))));
        assert_eq!(emu.cpu.pc, STRADDLING);

        emu.memory.write(STRADDLING + 2, &slli[2..]).unwrap();
        assert_eq!(emu.run(), Err(EmulationError::Kernel(())));
        assert_eq!(emu.cpu.registers.get(REG_A0), 6);
    }

    #[test]
    #[cfg(all(feature = "64-bit", feature = "a"))]
    fn lr_w_sign_extends() {
        let mut memory = SparseMemory::new();
        memory.write(0x1000, &rv_code! { lr.w a0, (a1); ecall }).unwrap();
        memory.write(0x2000, &0x8000_0000u32.to_le_bytes()).unwrap();

        let mut emu = emulator(0x1000, memory, ());
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
        let mut emu = emulator(0x1000, memory, ());

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
        let mut emu = emulator(0x1000, memory, ());
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

    /// A [`Memory`] with RAM below [`BoundedMemory::END`] and nothing above it. It implements no
    /// more of [`Memory`] than it has to.
    #[derive(Default)]
    struct BoundedMemory(SparseMemory);

    /// An access that reaches beyond the end of a [`BoundedMemory`], with its address.
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

    impl Memory for BoundedMemory {
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

        fn syscall<M: Memory>(
            &mut self,
            _: XWord,
            memory: &mut M,
            registers: &mut Registers,
            _: &mut (),
        ) -> Result<Syscall, SyscallError<(), M::Error>> {
            let [byte] = memory.load(registers.get(REG_A0))?;
            Ok(Syscall::Exit(byte as XWord))
        }
    }

    #[test]
    fn a_store_that_the_bus_fails_can_be_tried_again() {
        let mut memory = BoundedMemory::default();
        let code = rv_code! {
            addi a0, zero, 7;
            sb a0, 0(a1);
            add a0, zero, a1;
            ecall;
        };
        memory.write(0x1000, &code).unwrap();
        let mut emu = emulator(0x1000, memory, LoadKernel);
        emu.cpu.registers.set(REG_A1, BoundedMemory::END);

        // The store stops the run before it has changed anything.
        assert_eq!(emu.run(), Err(EmulationError::Memory(OutOfBounds(BoundedMemory::END))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (0x1004, 1));

        // With an address that exists, the run reaches the system call, which reads the byte.
        emu.cpu.registers.set(REG_A1, 0x2000);
        assert_eq!(emu.run(), Ok(7));
    }

    #[test]
    fn the_bus_can_fail_a_fetch_and_an_access_by_the_kernel() {
        // The last instruction that the memory has room for.
        let last = BoundedMemory::END - 4;
        let mut memory = BoundedMemory::default();
        memory.write(last, &rv!(addi a0, zero, 1).to_le_bytes()).unwrap();
        let mut emu = emulator(last, memory, LoadKernel);

        // Fetching it does not read past the end, but there is no instruction after it.
        assert_eq!(emu.run(), Err(EmulationError::Memory(OutOfBounds(BoundedMemory::END))));
        assert_eq!((emu.cpu.pc, emu.cpu.instret), (BoundedMemory::END, 1));

        // The kernel passes on the error of its own access.
        emu.memory.write(last, &rv!(ecall).to_le_bytes()).unwrap();
        emu.cpu.pc = last;
        emu.cpu.registers.set(REG_A0, BoundedMemory::END);
        assert_eq!(emu.run(), Err(EmulationError::Memory(OutOfBounds(BoundedMemory::END))));

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

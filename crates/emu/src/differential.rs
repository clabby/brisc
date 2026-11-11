//! Checks that [Cpu] and the stage pipeline of [StEmu] compute the same thing.
//!
//! Both engines start from the same memory image and are advanced in lock step. Program counter
//! and registers are compared after every step, and all written memory at the end.

use crate::{
    cfg::EmuConfig,
    elf::load_elf,
    st::StEmu,
    test_utils::RiscvTestKernel,
};
use brisc_hw::{
    cpu::{Cpu, Stop},
    memory::{Address, Memory, PageIndex, SparseMemory, PAGE_ADDRESS_SIZE, PAGE_SIZE},
    Word, XWord, REG_A0, REG_A7,
};
use std::{collections::BTreeSet, fs};

/// A [SparseMemory] that remembers which pages were written.
#[derive(Debug, Default)]
struct Recording {
    memory: SparseMemory,
    written: BTreeSet<PageIndex>,
}

impl Recording {
    fn record(&mut self, address: Address, len: usize) {
        if len > 0 {
            let last = address.wrapping_add(len as Address - 1);
            self.written.insert(address >> PAGE_ADDRESS_SIZE);
            self.written.insert(last >> PAGE_ADDRESS_SIZE);
            // Longer ranges only come from the ELF loader and do not wrap.
            let mut page = address >> PAGE_ADDRESS_SIZE;
            while page < last >> PAGE_ADDRESS_SIZE {
                self.written.insert(page);
                page += 1;
            }
        }
    }
}

impl Memory for Recording {
    fn load<const N: usize>(&mut self, address: Address) -> [u8; N] {
        self.memory.load(address)
    }

    fn store<const N: usize>(&mut self, address: Address, bytes: [u8; N]) -> bool {
        self.record(address, N);
        self.memory.store(address, bytes)
    }

    fn read(&mut self, address: Address, buf: &mut [u8]) {
        self.memory.read(address, buf)
    }

    fn write(&mut self, address: Address, data: &[u8]) {
        self.record(address, data.len());
        self.memory.write(address, data)
    }

    fn watch(&mut self, page: PageIndex) {
        self.memory.watch(page)
    }

    fn pop_dirty(&mut self) -> Option<PageIndex> {
        self.memory.pop_dirty()
    }

    fn page_count(&self) -> usize {
        self.memory.page_count()
    }
}

#[derive(Debug)]
struct Config;

impl EmuConfig<'_> for Config {
    type Memory = Recording;
    type Kernel = RiscvTestKernel;
    type Context = ();
}

/// The two engines, loaded with the same program.
struct Pair {
    pipeline: StEmu<'static, Config>,
    cpu: Cpu,
    memory: Recording,
}

/// How a [Pair] stopped.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    /// The step budget ran out.
    Running,
    /// Both engines stopped at an `ecall` with this syscall number.
    Ecall(XWord),
    /// Both engines refused to execute the instruction at the program counter.
    Fault,
}

impl Pair {
    fn new(pc: XWord, image: impl Fn(&mut Recording)) -> Self {
        let mut memories = [Recording::default(), Recording::default()];
        memories.iter_mut().for_each(image);
        let [first, second] = memories;
        let pipeline =
            StEmu::builder().with_pc(pc).with_memory(first).with_kernel(RiscvTestKernel).with_ctx(()).build();
        Self { pipeline, cpu: Cpu::new(pc), memory: second }
    }

    fn set_register(&mut self, index: usize, value: XWord) {
        self.pipeline.register.registers[index] = value;
        self.cpu.registers.set(index, value);
    }

    /// Advances both engines by up to `budget` instructions and compares their state. An `ecall`
    /// is left unexecuted by both.
    fn step(&mut self, budget: u64, context: &str) -> Outcome {
        let before = self.cpu.instret;
        let stop = self.cpu.run(&mut self.memory, budget);
        let mut retired = self.cpu.instret - before;
        if stop == Stop::Ecall {
            // The pipeline hands an `ecall` to its kernel within the cycle.
            retired -= 1;
        }

        for _ in 0..retired {
            self.pipeline.cycle().unwrap_or_else(|e| panic!("{context}: pipeline failed: {e:?}"));
        }
        let at = self.cpu.instret;
        assert_eq!(self.pipeline.register.pc, self.cpu.pc, "{context}: pc after {at} instructions");
        for index in 0..32 {
            assert_eq!(
                self.pipeline.register.registers[index],
                self.cpu.registers.get(index),
                "{context}: x{index} after {at} instructions, pc = {:#x}",
                self.cpu.pc,
            );
        }

        match stop {
            Stop::Budget => Outcome::Running,
            Stop::Ecall => Outcome::Ecall(self.cpu.registers.get(REG_A7)),
            Stop::Fault(fault) => {
                assert!(self.pipeline.cycle().is_err(), "{context}: only the cpu faulted: {fault}");
                Outcome::Fault
            }
        }
    }

    /// Lets both engines pass an `ecall` that asks for nothing.
    fn skip_ecall(&mut self) {
        self.pipeline.cycle().unwrap();
        self.cpu.pc = self.cpu.pc.wrapping_add(4);
    }

    fn assert_same_memory(&mut self, context: &str) {
        let pages = self.memory.written.union(&self.pipeline.memory.written).copied().collect::<Vec<_>>();
        for page in pages {
            let (mut a, mut b) = ([0; PAGE_SIZE], [0; PAGE_SIZE]);
            self.pipeline.memory.read(page << PAGE_ADDRESS_SIZE, &mut a);
            self.memory.read(page << PAGE_ADDRESS_SIZE, &mut b);
            assert!(a == b, "{context}: memory differs in page {page:#x}");
        }
    }
}

/// Whether the enabled features cover the ISA that the test program `name` is built for.
fn supported(name: &str) -> bool {
    let extension = |name: &str| match name.as_bytes()[0] {
        b'i' => true,
        b'm' => cfg!(feature = "m"),
        b'a' => cfg!(feature = "a"),
        b'c' => cfg!(feature = "c"),
        _ => false,
    };
    let imac = cfg!(all(feature = "m", feature = "a", feature = "c"));

    if name.ends_with(".dump") {
        false
    } else if let Some(name) = name.strip_prefix("rv32u") {
        extension(name)
    } else if let Some(name) = name.strip_prefix("rv64u") {
        cfg!(feature = "64-bit") && extension(name)
    } else if name.starts_with("rs-32bit-") {
        !cfg!(feature = "64-bit") && imac
    } else if name.starts_with("rs-64bit-") {
        cfg!(feature = "64-bit") && imac
    } else {
        false
    }
}

#[test]
fn test_programs_match() {
    // Single steps exercise the precise interpreter loop; the larger sizes exercise the fast
    // loop and the hand-over between the two, which happens at 2048 remaining instructions.
    const SINGLE_STEPS: u64 = 5000;
    const BUDGETS: [u64; 10] = [1, 2, 3, 61, 257, 2047, 2048, 2049, 4999, 100_003];

    let mut programs = fs::read_dir("../../rv-tests/bin")
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| supported(&path.file_name().unwrap().to_string_lossy()))
        .collect::<Vec<_>>();
    programs.sort();

    for program in programs {
        let name = program.file_name().unwrap().to_string_lossy().into_owned();
        let elf = fs::read(&program).unwrap();
        let (_, entry) = load_elf::<SparseMemory>(&elf).unwrap();
        let mut pair = Pair::new(entry, |memory| *memory = load_elf(&elf).unwrap().0);

        let mut steps = 0;
        loop {
            let budget =
                if steps < SINGLE_STEPS { 1 } else { BUDGETS[steps as usize % BUDGETS.len()] };
            steps += 1;
            match pair.step(budget, &name) {
                Outcome::Running => {}
                Outcome::Ecall(0x5D) => break,
                Outcome::Ecall(_) => pair.skip_ecall(),
                Outcome::Fault => panic!("{name}: fault at {:#x}", pair.cpu.pc),
            }
        }
        assert_eq!(pair.cpu.registers.get(REG_A0), 0, "{name}: exit code");
        pair.assert_same_memory(&name);
    }
}

/// A xorshift64 generator.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn random_instructions_match() {
    /// Where the random code is placed: the last two thirds of a page and the start of the next,
    /// so that some instructions straddle the boundary.
    const CODE: XWord = 0x1_0000 + 0x1000 / 3 * 2;
    const WORDS: usize = 384;
    const CASES: u64 = 20_000;
    const STEPS: u64 = 48;
    /// The major opcodes of the base ISA and the M, A and 64-bit extensions.
    const OPCODES: [Word; 14] = [
        0b000_0011, 0b000_1111, 0b001_0011, 0b001_0111, 0b001_1011, 0b010_0011, 0b010_1111,
        0b011_0011, 0b011_0111, 0b011_1011, 0b110_0011, 0b110_0111, 0b110_1111, 0b111_0011,
    ];

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for case in 0..CASES {
        let context = format!("case {case}");

        let mut code = Vec::with_capacity(4 * WORDS);
        for _ in 0..WORDS {
            let bits = rng.next();
            let mut word = (bits >> 32) as Word;
            match bits % 8 {
                // A word with a valid major opcode, usually with small register numbers so that
                // instructions depend on each other, and sometimes with a small function field.
                0..=5 => {
                    word = word & !0x7f | OPCODES[(bits >> 8) as usize % OPCODES.len()];
                    if bits & (1 << 16) != 0 {
                        word &= !0x0181_8c00;
                    }
                    if bits & (1 << 17) != 0 {
                        word &= !0xfc00_0000;
                    }
                }
                // Two 16-bit parcels, which are compressed instructions unless they end in 0b11.
                6 => {}
                // An environment call ends the case.
                _ => word = 0b111_0011,
            }
            code.extend_from_slice(&word.to_le_bytes());
        }

        let mut pair = Pair::new(CODE + (rng.next() % 64) as XWord * 2, |memory| memory.write(CODE, &code));
        for index in 1..32 {
            // Small values and addresses in and around the code.
            let bits = rng.next();
            let value = match bits % 4 {
                0 => (bits >> 8) as XWord % 64,
                1 => CODE.wrapping_add((bits >> 8) as XWord % 0x800).wrapping_sub(0x100),
                2 => ((bits >> 8) as XWord % 16).wrapping_neg(),
                _ => (bits >> 8) as XWord,
            };
            pair.set_register(index, value);
        }

        for _ in 0..STEPS {
            if pair.step(1, &context) != Outcome::Running {
                break;
            }
        }
        pair.assert_same_memory(&context);
    }
}

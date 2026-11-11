//! Checks that [Cpu] and the stage pipeline compute the same thing.
//!
//! Both engines start from the same memory image and are advanced in lock step. Program counter
//! and registers are compared after every step, and all written memory at the end.

use crate::elf::load_elf;
use brisc_hw::{
    cpu::{Cpu, Stop},
    errors::{PipelineError, PipelineResult},
    memory::{Address, Memory, PageIndex, SparseMemory, PAGE_ADDRESS_SIZE, PAGE_SIZE},
    pipeline::{
        decode_instruction, execute, instruction_fetch, mem_access, writeback, PipelineRegister,
    },
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

/// The two engines, loaded with the same program.
struct Pair {
    pipeline: PipelineRegister,
    pipeline_memory: Recording,
    cpu: Cpu,
    cpu_memory: Recording,
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
        let [pipeline_memory, cpu_memory] = memories;
        Self { pipeline: PipelineRegister::new(pc), pipeline_memory, cpu: Cpu::new(pc), cpu_memory }
    }

    fn set_register(&mut self, index: usize, value: XWord) {
        self.pipeline.registers[index] = value;
        self.cpu.registers.set(index, value);
    }

    /// Runs one instruction through the stages. An `ecall` is reported as
    /// [PipelineError::SyscallException] and leaves the program counter on the instruction.
    fn cycle(&mut self) -> PipelineResult<()> {
        let (r, memory) = (&mut self.pipeline, &mut self.pipeline_memory);
        instruction_fetch(r, memory)
            .and_then(|_| decode_instruction(r))
            .and_then(|_| execute(r))
            .and_then(|_| mem_access(r, memory))
            .and_then(|_| writeback(r))?;
        r.advance();
        Ok(())
    }

    /// Advances both engines by up to `budget` instructions and compares their state. Both stop
    /// in front of an `ecall`.
    fn step(&mut self, budget: u64, context: &str) -> Outcome {
        let before = self.cpu.instret;
        let stop = self.cpu.run(&mut self.cpu_memory, budget);
        let mut retired = self.cpu.instret - before;
        if stop == Stop::Ecall {
            // The cpu counts the `ecall` as retired.
            retired -= 1;
        }

        for _ in 0..retired {
            self.cycle().unwrap_or_else(|e| panic!("{context}: pipeline failed: {e:?}"));
        }
        let at = self.cpu.instret;
        assert_eq!(self.pipeline.pc, self.cpu.pc, "{context}: pc after {at} instructions");
        for index in 0..32 {
            assert_eq!(
                self.pipeline.registers[index],
                self.cpu.registers.get(index),
                "{context}: x{index} after {at} instructions, pc = {:#x}",
                self.cpu.pc,
            );
        }

        match stop {
            Stop::Budget => Outcome::Running,
            Stop::Ecall => {
                assert!(
                    matches!(self.cycle(), Err(PipelineError::SyscallException(_))),
                    "{context}: only the cpu stopped at an ecall"
                );
                Outcome::Ecall(self.cpu.registers.get(REG_A7))
            }
            Stop::Fault(fault) => {
                // The pipeline executes a store-conditional of an unsupported size as one that
                // fails, where the cpu rejects every atomic instruction of such a size.
                #[cfg(feature = "a")]
                let stricter = matches!(fault, brisc_hw::cpu::Fault::BadAmoSize(_));
                #[cfg(not(feature = "a"))]
                let stricter = false;
                assert!(
                    self.cycle().is_err() || stricter,
                    "{context}: only the cpu faulted: {fault}"
                );
                Outcome::Fault
            }
        }
    }

    /// Lets both engines pass an `ecall` that asks for nothing.
    fn skip_ecall(&mut self) {
        self.pipeline.advance();
        self.cpu.pc = self.cpu.pc.wrapping_add(4);
    }

    fn assert_same_memory(&mut self, context: &str) {
        let pages = self
            .cpu_memory
            .written
            .union(&self.pipeline_memory.written)
            .copied()
            .collect::<Vec<_>>();
        for page in pages {
            let (mut a, mut b) = ([0; PAGE_SIZE], [0; PAGE_SIZE]);
            self.pipeline_memory.read(page << PAGE_ADDRESS_SIZE, &mut a);
            self.cpu_memory.read(page << PAGE_ADDRESS_SIZE, &mut b);
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

/// Returns a random word that is likely to decode: a major opcode of the base ISA or of the M, A
/// and 64-bit extensions with function fields biased towards the defined ones, two random 16-bit
/// parcels, or occasionally an `ecall`.
fn random_word(rng: &mut Rng) -> Word {
    const OPCODES: [Word; 14] = [
        0b000_0011, 0b000_1111, 0b001_0011, 0b001_0111, 0b001_1011, 0b010_0011, 0b010_1111,
        0b011_0011, 0b011_0111, 0b011_1011, 0b110_0011, 0b110_0111, 0b110_1111, 0b111_0011,
    ];
    const FUNCT7: [Word; 4] = [0x00, 0x00, 0x20, 0x01];
    const FUNCT5: [Word; 11] = [
        0b00010, 0b00011, 0b00001, 0b00000, 0b00100, 0b01100, 0b01000, 0b10000, 0b10100, 0b11000,
        0b11100,
    ];

    let bits = rng.next();
    let field = |shift: u32, width: u32| (bits >> shift) as Word & ((1 << width) - 1);
    let mut word = (bits >> 32) as Word;
    match field(0, 5) {
        0..=24 => {}
        25..=30 => return word,
        _ => return 0b111_0011,
    }

    let opcode = OPCODES[field(5, 8) as usize % OPCODES.len()];
    word = word & !0x7f | opcode;
    if field(13, 3) != 0 {
        word = word & 0x01ff_ffff | FUNCT7[field(16, 2) as usize] << 25;
    }
    if field(18, 2) != 0 {
        // Registers `x0` to `x7`, so that instructions depend on each other.
        word &= !0x018c_0c00;
    }
    match opcode {
        // Stores: a defined width most of the time.
        0b010_0011 => word &= !0x4000,
        // Atomics: a defined function and usually a defined width.
        0b010_1111 => {
            word = word & 0x07ff_ffff | FUNCT5[field(20, 4) as usize % FUNCT5.len()] << 27;
            if field(24, 3) != 0 {
                word = word & !0x7000 | (2 + field(27, 1)) << 12;
            }
        }
        // Branches and jumps: short and forward, to stay within the code. Backward transfers
        // come from `jalr` through registers that point into the code.
        0b110_0011 => word &= !0xf800_0080,
        0b110_1111 => word &= !0xf01f_f000,
        _ => {}
    }
    word
}

#[test]
fn random_instructions_match() {
    /// Where the random code is placed: the end of one page and the start of the next, so that
    /// some instructions straddle the boundary.
    const CODE: XWord = 0x1_0c00;
    const WORDS: usize = 384;
    const CASES: u64 = 20_000;
    const STEPS: u64 = 48;

    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut retired = 0;
    for case in 0..CASES {
        let context = format!("case {case}");

        let code = (0..WORDS).flat_map(|_| random_word(&mut rng).to_le_bytes()).collect::<Vec<_>>();
        let entry = CODE + (rng.next() % 64) as XWord * if cfg!(feature = "c") { 2 } else { 4 };
        let mut pair = Pair::new(entry, |memory| memory.write(CODE, &code));
        for index in 1..32 {
            // Small values, addresses in and around the code, and arbitrary values.
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
        retired += pair.cpu.instret;
        pair.assert_same_memory(&context);
    }

    // Enough words must have decoded for the comparison to mean anything.
    assert!(retired > CASES, "only {retired} instructions were compared");
}

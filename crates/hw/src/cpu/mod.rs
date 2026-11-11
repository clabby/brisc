//! The processor: register file, program counter and interpreter.
//!
//! Instructions are decoded once and cached per page of guest memory, with one slot for every
//! two bytes of the page. The interpreter runs over the slots of the current page and
//! returns here to change pages, to have slots decoded, and to drop decoded instructions that the
//! guest has overwritten.

use crate::memory::{Address, Memory, PageIndex, PAGE_ADDRESS_MASK, PAGE_ADDRESS_SIZE, PAGE_SIZE};
use alloc::{boxed::Box, vec, vec::Vec};
use brisc_isa::{InstructionDecodeError, Word, XWord};
use core::fmt;
use hashbrown::HashMap;
use thiserror::Error;

mod exec;
use exec::{execute, Exit};

mod op;
use op::{lower, narrow, wide, Op};

/// The number of instruction slots in a page: an instruction can start at every second byte.
const SLOTS: usize = PAGE_SIZE / 2;

/// The decoded form of one page of guest memory.
///
/// The [SLOTS] instruction slots are followed by two [Op::PAGE_END] slots, which is as far as
/// the last instruction of the page can advance.
type CodePage = [Op; SLOTS + 2];

/// The reason [Cpu::run] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// The instruction at the program counter is an `ecall`.
    ///
    /// It counts as retired, and the program counter still points at it: to continue after
    /// servicing the call, advance [Cpu::pc] by four bytes.
    Ecall,
    /// The requested number of instructions has been retired.
    Budget,
    /// The instruction at the program counter cannot be executed. It changed no state.
    Fault(Fault),
}

/// A condition that prevents an instruction from executing.
#[derive(Error, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// The word at the program counter is not an instruction.
    #[error(transparent)]
    Decode(#[from] InstructionDecodeError),
    /// An atomic instruction has an access size other than a word or, on 64-bit, a doubleword.
    #[cfg(feature = "a")]
    #[error("Bad AMO size: {0}")]
    BadAmoSize(u8),
    /// The address of an atomic instruction is not aligned to its access size.
    #[cfg(feature = "a")]
    #[error("Unaligned atomic memory access.")]
    UnalignedAmo,
}

/// The integer register file.
///
/// `x0` always reads as zero.
#[derive(Clone, PartialEq, Eq)]
pub struct Registers(
    /// Slots 0 to 31 hold `x0` to `x31`. Decoded instructions that name `x0` as their destination
    /// write to slot 32 instead, which keeps slot 0 zero without a check on every write. The
    /// length is a power of two so that a masked index is always in bounds.
    [XWord; 64],
);

impl Registers {
    /// Returns the value of register `index`.
    ///
    /// ## Panics
    ///
    /// Panics if `index` is not below 32.
    pub fn get(&self, index: usize) -> XWord {
        self.0[..32][index]
    }

    /// Sets register `index` to `value`. Writes to `x0` are discarded.
    ///
    /// ## Panics
    ///
    /// Panics if `index` is not below 32.
    pub fn set(&mut self, index: usize, value: XWord) {
        if index != 0 {
            self.0[..32][index] = value;
        }
    }
}

impl fmt::Debug for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.0[..32]).finish()
    }
}

/// The instructions decoded so far, by page.
#[derive(Default)]
struct CodeCache {
    pages: Vec<Box<CodePage>>,
    /// The position in [Self::pages] of each page that has been decoded from.
    index: HashMap<PageIndex, usize>,
    /// The page most recently looked up and its position in [Self::pages].
    last: Option<(PageIndex, usize)>,
}

impl CodeCache {
    /// Returns the position of the decoded form of `page`, creating it if needed.
    fn position(&mut self, page: PageIndex) -> usize {
        if let Some((last, position)) = self.last {
            if last == page {
                return position;
            }
        }

        let position = *self.index.entry(page).or_insert_with(|| {
            let mut code = vec![Op::DECODE; SLOTS + 2];
            code[SLOTS..].fill(Op::PAGE_END);
            self.pages
                .push(code.into_boxed_slice().try_into().expect("a page has SLOTS + 2 slots"));
            self.pages.len() - 1
        });
        self.last = Some((page, position));
        position
    }

    /// Decodes the instructions of `page` from `slot` up to the first one that is already decoded
    /// or that execution cannot fall through.
    #[cold]
    fn decode<M: Memory>(&mut self, memory: &mut M, page: PageIndex, mut slot: usize) {
        let position = self.position(page);
        memory.watch(page);

        while slot < SLOTS && self.pages[position][slot].code == narrow::DECODE {
            let address = (page << PAGE_ADDRESS_SIZE).wrapping_add((slot as Address) << 1);
            let op = lower(Word::from_le_bytes(memory.load(address))).unwrap_or(Op::ILLEGAL);
            let is_wide = op.code & op::WIDE != 0;
            self.pages[position][slot] = op;

            if is_wide && slot == SLOTS - 1 {
                // The instruction continues on the next page.
                memory.watch(page.wrapping_add(1) & (PageIndex::MAX >> PAGE_ADDRESS_SIZE));
            }
            if matches!(
                op.code,
                narrow::ILLEGAL | narrow::JAL | wide::JAL | narrow::JALR | wide::JALR
            ) {
                break;
            }
            slot += if is_wide { 2 } else { 1 };
        }
    }

    /// Forgets the instructions decoded from the bytes of `page`.
    #[cold]
    fn invalidate(&mut self, page: PageIndex) {
        if let Some(position) = self.index.get(&page) {
            self.pages[*position][..SLOTS].fill(Op::DECODE);
        }
        // The last instruction of the previous page may continue on this one.
        let previous = page.wrapping_sub(1) & (PageIndex::MAX >> PAGE_ADDRESS_SIZE);
        if let Some(position) = self.index.get(&previous) {
            self.pages[*position][SLOTS - 1] = Op::DECODE;
        }
    }
}

impl fmt::Debug for CodeCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodeCache").field("pages", &self.pages.len()).finish_non_exhaustive()
    }
}

/// A RISC-V hart.
#[derive(Debug)]
pub struct Cpu {
    /// The integer registers.
    pub registers: Registers,
    /// The address of the next instruction to execute. Instructions start at even addresses; the
    /// lowest bit is ignored.
    pub pc: XWord,
    /// The number of instructions retired.
    pub instret: u64,
    /// The address registered by the last load-reserved, if a store-conditional has not consumed
    /// it yet.
    #[cfg(feature = "a")]
    reservation: Option<Address>,
    code: CodeCache,
}

impl Cpu {
    /// Creates a hart with zeroed registers that starts executing at `pc`.
    pub fn new(pc: XWord) -> Self {
        Self {
            registers: Registers([0; 64]),
            pc,
            instret: 0,
            #[cfg(feature = "a")]
            reservation: None,
            code: CodeCache::default(),
        }
    }

    /// Executes instructions from `memory` until `budget` of them have been retired, or until an
    /// `ecall` or a [Fault] stops execution earlier.
    ///
    /// Decoded instructions stay coherent with `memory` as long as all writes to it go through
    /// the [Memory] trait.
    pub fn run<M: Memory>(&mut self, memory: &mut M, budget: u64) -> Stop {
        let mut remaining = budget;

        let stop = loop {
            while let Some(page) = memory.pop_dirty() {
                self.code.invalidate(page);
            }
            if remaining == 0 {
                break Stop::Budget;
            }

            let page = self.pc >> PAGE_ADDRESS_SIZE;
            let slot = (self.pc as usize & PAGE_ADDRESS_MASK) >> 1;
            let position = self.code.position(page);
            let code = &self.code.pages[position];
            let base = page << PAGE_ADDRESS_SIZE;

            let (exit, pc) = if remaining >= SLOTS as u64 {
                execute::<M, false>(
                    &mut self.registers,
                    #[cfg(feature = "a")]
                    &mut self.reservation,
                    memory,
                    code,
                    base,
                    slot,
                    &mut remaining,
                )
            } else {
                execute::<M, true>(
                    &mut self.registers,
                    #[cfg(feature = "a")]
                    &mut self.reservation,
                    memory,
                    code,
                    base,
                    slot,
                    &mut remaining,
                )
            };
            self.pc = pc;

            match exit {
                Exit::Leave => {}
                Exit::Decode => {
                    self.code.decode(memory, page, (pc as usize & PAGE_ADDRESS_MASK) >> 1)
                }
                Exit::Ecall => break Stop::Ecall,
                Exit::Illegal => {
                    let fault = lower(Word::from_le_bytes(memory.load(pc)))
                        .expect_err("an illegal slot holds a word that does not decode");
                    break Stop::Fault(fault);
                }
                #[cfg(feature = "a")]
                Exit::Fault(fault) => break Stop::Fault(fault),
            }
        };

        self.instret += budget - remaining;
        stop
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::memory::SparseMemory;
    use brisc_isa::{REG_A0, REG_A1, REG_ZERO};

    const BASE: XWord = 0x1000;

    fn addi(rd: usize, rs1: usize, imm: i32) -> Word {
        (imm as Word & 0xfff) << 20 | (rs1 as Word) << 15 | (rd as Word) << 7 | 0b001_0011
    }

    fn sw(rs2: usize, rs1: usize, imm: i32) -> Word {
        let imm = imm as Word & 0xfff;
        (imm >> 5) << 25 |
            (rs2 as Word) << 20 |
            (rs1 as Word) << 15 |
            0b010 << 12 |
            (imm & 0x1f) << 7 |
            0b010_0011
    }

    fn bne(rs1: usize, rs2: usize, offset: i32) -> Word {
        let imm = offset as Word;
        (imm >> 12 & 1) << 31 |
            (imm >> 5 & 0x3f) << 25 |
            (rs2 as Word) << 20 |
            (rs1 as Word) << 15 |
            0b001 << 12 |
            (imm >> 1 & 0xf) << 8 |
            (imm >> 11 & 1) << 7 |
            0b110_0011
    }

    fn jal(rd: usize, offset: i32) -> Word {
        let imm = offset as Word;
        (imm >> 20 & 1) << 31 |
            (imm >> 1 & 0x3ff) << 21 |
            (imm >> 11 & 1) << 20 |
            (imm >> 12 & 0xff) << 12 |
            (rd as Word) << 7 |
            0b110_1111
    }

    const ECALL: Word = 0b111_0011;

    fn machine(address: XWord, program: &[Word]) -> (Cpu, SparseMemory) {
        let mut memory = SparseMemory::new();
        for (i, word) in program.iter().enumerate() {
            memory.store(address.wrapping_add(4 * i as XWord), word.to_le_bytes());
        }
        (Cpu::new(address), memory)
    }

    /// Counts `a0` down from `n` to zero, adding 3 to `a1` each time, then makes an `ecall`.
    fn countdown(address: XWord, n: i32) -> (Cpu, SparseMemory) {
        machine(
            address,
            &[
                addi(REG_A0, REG_ZERO, n),
                addi(REG_A1, REG_A1, 3),
                addi(REG_A0, REG_A0, -1),
                bne(REG_A0, REG_ZERO, -8),
                ECALL,
            ],
        )
    }

    #[test]
    fn runs_to_ecall() {
        let (mut cpu, mut memory) = countdown(BASE, 1000);
        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
        assert_eq!(cpu.registers.get(REG_A0), 0);
        assert_eq!(cpu.registers.get(REG_A1), 3000);
        assert_eq!(cpu.pc, BASE + 16);
        assert_eq!(cpu.instret, 1 + 3 * 1000 + 1);
    }

    #[test]
    fn retires_exactly_the_budget() {
        let (mut whole, mut memory) = countdown(BASE, 2000);
        assert_eq!(whole.run(&mut memory, u64::MAX), Stop::Ecall);

        for budget in [1, 2, 3, 7, SLOTS as u64 - 1, SLOTS as u64, SLOTS as u64 + 1, 5000] {
            let (mut cpu, mut memory) = countdown(BASE, 2000);
            let mut stop = Stop::Budget;
            while stop == Stop::Budget {
                let before = cpu.instret;
                stop = cpu.run(&mut memory, budget);
                assert!(cpu.instret - before == budget || stop == Stop::Ecall);
            }
            assert_eq!(stop, Stop::Ecall);
            assert_eq!(cpu.instret, whole.instret);
            assert_eq!(cpu.registers, whole.registers);
            assert_eq!(cpu.pc, whole.pc);
        }
    }

    #[test]
    fn zero_budget_executes_nothing() {
        let (mut cpu, mut memory) = countdown(BASE, 5);
        assert_eq!(cpu.run(&mut memory, 0), Stop::Budget);
        assert_eq!((cpu.pc, cpu.instret), (BASE, 0));
    }

    #[test]
    fn x0_stays_zero() {
        let (mut cpu, mut memory) = machine(
            BASE,
            &[addi(REG_ZERO, REG_ZERO, 5), jal(REG_ZERO, 4), addi(REG_A0, REG_ZERO, 1), ECALL],
        );
        cpu.registers.set(REG_ZERO, 9);
        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
        assert_eq!(cpu.registers.get(REG_ZERO), 0);
        assert_eq!(cpu.registers.get(REG_A0), 1);
    }

    #[test]
    fn executes_across_a_page_boundary() {
        // The second instruction straddles the boundary when the program starts six bytes
        // before it, and starts on it when the program starts four bytes before it.
        for start in [2 * BASE - 6, 2 * BASE - 4] {
            let (mut cpu, mut memory) = countdown(start, 10);
            assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
            assert_eq!(cpu.registers.get(REG_A1), 30);
            assert_eq!(cpu.pc, start + 16);
        }
    }

    #[test]
    fn overwritten_instructions_are_decoded_again() {
        // Runs the instruction at index `patched` once, overwrites it with the word in `t0`
        // through the address in `t1`, and runs it again.
        let (t0, t1) = (5, 6);
        let patched = 3;
        let program = [
            addi(REG_A1, REG_ZERO, 2),
            addi(REG_A1, REG_A1, -1),
            jal(REG_ZERO, 4),
            addi(REG_A0, REG_A0, 1),
            sw(t0, t1, 0),
            bne(REG_A1, REG_ZERO, -16),
            ECALL,
        ];

        // Once with the patched instruction inside a page and once with it straddling the
        // boundary to the next page.
        for start in [BASE, 2 * BASE - 4 * patched - 2] {
            let (mut cpu, mut memory) = machine(start, &program);
            cpu.registers.set(t0, addi(REG_A0, REG_A0, 100) as XWord);
            cpu.registers.set(t1, start + 4 * patched);
            assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
            assert_eq!(cpu.registers.get(REG_A0), 101);
        }
    }

    #[test]
    fn external_writes_are_seen_by_the_next_run() {
        let (mut cpu, mut memory) = machine(BASE, &[addi(REG_A0, REG_A0, 1), ECALL]);
        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
        memory.write(BASE, &addi(REG_A0, REG_A0, 7).to_le_bytes());
        cpu.pc = BASE;
        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);
        assert_eq!(cpu.registers.get(REG_A0), 8);
    }

    #[test]
    fn undecodable_word_faults_without_side_effects() {
        let (mut cpu, mut memory) = machine(BASE, &[addi(REG_A0, REG_ZERO, 1), 0xffff_ffff]);
        let stop = cpu.run(&mut memory, u64::MAX);
        assert!(matches!(stop, Stop::Fault(Fault::Decode(_))));
        assert_eq!((cpu.pc, cpu.instret), (BASE + 4, 1));
        // The fault is reported again when execution is retried.
        assert_eq!(cpu.run(&mut memory, u64::MAX), stop);
        assert_eq!((cpu.pc, cpu.instret), (BASE + 4, 1));
    }
}

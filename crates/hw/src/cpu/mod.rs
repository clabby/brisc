//! The processor: register file, program counter and interpreter.
//!
//! Instructions are decoded once and cached per page of guest memory, with one slot for every
//! two bytes of the page. The interpreter runs over the slots of the current page and
//! returns here to change pages, to have slots decoded, and to drop decoded instructions that the
//! guest has overwritten.

#[cfg(feature = "a")]
use crate::memory::Address;
use crate::memory::{page_base, page_index, Memory};
use brisc_isa::{Word, XWord};

mod code;
use code::{slot_of, CodeCache, SLOTS};

mod exec;
use exec::{execute, Exit};

mod op;
use op::decode;

mod registers;
pub use registers::Registers;

mod stop;
pub use stop::{Fault, Stop};

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
            registers: Registers::new(),
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

            let page = page_index(self.pc);
            let slot = slot_of(self.pc);
            let position = self.code.position(page);
            let code = &self.code.pages[position];
            let base = page_base(page);

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
                Exit::Decode => self.code.decode(memory, page, slot_of(pc)),
                Exit::Ecall => break Stop::Ecall,
                Exit::Illegal => {
                    let fault = decode(Word::from_le_bytes(memory.load(pc)))
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

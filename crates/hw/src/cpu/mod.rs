//! The processor: register file, program counter and interpreter.
//!
//! Instructions are decoded once and cached per page of guest memory, with one slot for every
//! two bytes of the page. The interpreter runs over the slots of the current page and
//! returns here to change pages, to have slots decoded, and to drop decoded instructions that the
//! guest has overwritten.

#[cfg(feature = "a")]
use crate::bus::Address;
use crate::bus::{page_base, page_index, Bus};
use brisc_isa::XWord;

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
///
/// The hart executes the unprivileged instructions of the enabled instruction sets. Of the
/// privileged architecture it models only what programs written for a bare hart run into:
///
/// - `ecall` and `ebreak` stop [Cpu::run] with [Stop::Ecall] and [Stop::Ebreak], for whoever drives
///   the hart to act on.
/// - Every control and status register reads as zero. Writes are ignored, except that an
///   instruction that writes to a read-only register is a [Fault::ReadOnlyCsr]. `unimp`, which
///   compilers emit where execution must not continue, is such an instruction.
/// - `mret`, `sret`, `wfi` and `sfence.vma` do nothing, because there are no privilege levels,
///   interrupts or address translation. `fence` and `fence.i` do nothing either: there is one hart,
///   and decoded instructions follow writes to the bus without being told to.
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

    /// Executes instructions from `bus` until `budget` of them have been retired, or until an
    /// `ecall`, an `ebreak`, a [Fault] or an error of the bus stops execution earlier.
    ///
    /// The instructions that the hart has decoded stay coherent with `bus` as long as the bus
    /// reports the writes to them, as [Bus] describes.
    pub fn run<B: Bus>(&mut self, bus: &mut B, budget: u64) -> Stop<B::Error> {
        let mut remaining = budget;

        let stop = loop {
            while let Some(page) = bus.pop_dirty() {
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
                execute::<B, false>(
                    &mut self.registers,
                    #[cfg(feature = "a")]
                    &mut self.reservation,
                    bus,
                    code,
                    base,
                    slot,
                    &mut remaining,
                )
            } else {
                execute::<B, true>(
                    &mut self.registers,
                    #[cfg(feature = "a")]
                    &mut self.reservation,
                    bus,
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
                    if let Err(error) = self.code.decode(bus, page, slot_of(pc)) {
                        break Stop::Bus(error);
                    }
                }
                Exit::Ecall => break Stop::Ecall,
                Exit::Ebreak => break Stop::Ebreak,
                Exit::Illegal(word) => {
                    let fault = decode(word)
                        .expect_err("an illegal slot holds a word that does not decode");
                    break Stop::Fault(fault);
                }
                #[cfg(feature = "a")]
                Exit::Fault(fault) => break Stop::Fault(fault),
                Exit::Bus(error) => break Stop::Bus(error),
            }
        };

        self.instret += budget - remaining;
        stop
    }
}

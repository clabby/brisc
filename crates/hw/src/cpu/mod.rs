//! The processor: register file, program counter and interpreter.
//!
//! Instructions are decoded once and cached per page of guest memory, with one slot for every
//! two bytes of the page. The interpreter runs over the slots of the current page and
//! returns here to change pages, to have slots decoded, and to drop decoded instructions that the
//! guest has overwritten.

#[cfg(feature = "a")]
use crate::memory::Address;
use crate::memory::{page_base, page_index, Memory};
use brisc_isa::XWord;

mod code;
use code::{slot_of, CodeCache, SLOTS};

#[cfg(feature = "jit")]
mod jit;
#[cfg(feature = "jit")]
pub use jit::{Compilation, JitConfig, JitStats};
#[cfg(all(test, feature = "jit"))]
mod jit_test;

mod exec;
use exec::{execute, execute_weighted, Exit};

mod op;
use op::decode;

mod registers;
pub use registers::Registers;

mod stop;
pub use stop::{Fault, Stop};

mod trace;
pub use trace::{Step, Tracer};

mod weights;
pub use weights::Weights;
use weights::UNWEIGHED;

/// A RISC-V hart.
///
/// The hart executes the unprivileged instructions of the enabled instruction sets. Of the
/// privileged architecture it models only what programs written for a bare hart run into:
///
/// - `ecall` and `ebreak` stop [`Cpu::run`] with [`Stop::Ecall`] and [`Stop::Ebreak`], for whoever
///   drives the hart to act on.
/// - Every control and status register reads as zero. Writes are ignored, except that an
///   instruction that writes to a read-only register is a [`Fault::ReadOnlyCsr`]. `unimp`, which
///   compilers emit where execution must not continue, is such an instruction.
/// - `mret`, `sret`, `wfi` and `sfence.vma` do nothing, because there are no privilege levels,
///   interrupts or address translation. `fence` and `fence.i` do nothing either: there is one hart,
///   and decoded instructions follow writes to the memory without being told to.
#[derive(Debug)]
pub struct Cpu {
    /// The integer registers.
    pub registers: Registers,
    /// The address of the next instruction to execute. Instructions start at even addresses; the
    /// lowest bit is ignored.
    pub pc: XWord,
    /// The number of instructions retired.
    pub instret: u64,
    /// What the instructions retired have cost, in the units of the budget of [`Cpu::run`]: the
    /// sum of the weights that applied as each instruction retired. Without [`Weights`] it is
    /// [`Cpu::instret`].
    pub spent: u64,
    /// The address registered by the last load-reserved, if a store-conditional has not consumed
    /// it yet.
    #[cfg(feature = "a")]
    reservation: Option<Address>,
    weights: Option<Weights>,
    code: CodeCache,
    #[cfg(feature = "jit")]
    jit: jit::JitState,
}

impl Cpu {
    /// Creates a hart with zeroed registers that starts executing at `pc`.
    pub fn new(pc: XWord) -> Self {
        Self {
            registers: Registers::new(),
            pc,
            instret: 0,
            spent: 0,
            #[cfg(feature = "a")]
            reservation: None,
            weights: None,
            code: CodeCache::default(),
            #[cfg(feature = "jit")]
            jit: jit::JitState::new(),
        }
    }

    /// Sets what instructions cost against the budget of [`Cpu::run`], or makes every instruction
    /// cost one unit with None. A hart starts without weights.
    pub fn set_weights(&mut self, weights: Option<Weights>) {
        if self.weights != weights {
            #[cfg(feature = "jit")]
            self.jit.reset();
            self.weights = weights;
        }
    }

    /// Returns what instructions cost, or None if each costs one unit.
    pub const fn weights(&self) -> Option<&Weights> {
        self.weights.as_ref()
    }

    /// Executes instructions from `memory` until `budget` units have been spent, or until an
    /// `ecall`, an `ebreak`, a [`Fault`] or an error of the memory stops execution earlier.
    ///
    /// An instruction costs one unit, or the weight that [`Cpu::set_weights`] gave to its opcode.
    /// The hart stops with [`Stop::Budget`], before executing an instruction, if the budget that
    /// remains does not cover it.
    ///
    /// The instructions that the hart has decoded stay coherent with `memory` as long as the memory
    /// reports the writes to them, as [`Memory`] describes.
    pub fn run<M: Memory>(&mut self, memory: &mut M, budget: u64) -> Stop<M::Error> {
        self.run_traced(memory, budget, &mut (), &mut ())
    }

    /// Does what [`Cpu::run`] does, and shows every instruction to `tracer` before executing it,
    /// with `ctx`, which the hart does not look at.
    ///
    /// The instructions are interpreted one by one, even if the hart would compile them without a
    /// tracer. A `tracer` that does not trace, like `()`, makes this the same as [`Cpu::run`].
    pub fn run_traced<M: Memory, C, T: Tracer<M, C>>(
        &mut self,
        memory: &mut M,
        budget: u64,
        tracer: &mut T,
        ctx: &mut C,
    ) -> Stop<M::Error> {
        let mut remaining = budget;
        // The instructions retired, as far as the budget does not tell: see [`execute`].
        let mut retired = 0;
        let weighted = self.weights.is_some();

        let stop = loop {
            while let Some(page) = memory.pop_dirty() {
                #[allow(unused_variables)]
                let position = self.code.invalidate(page);
                #[cfg(feature = "jit")]
                if let Some(position) = position {
                    self.jit.invalidate(position);
                }
            }
            if remaining == 0 {
                break Stop::Budget;
            }

            let page = page_index(self.pc);
            let slot = slot_of(self.pc);
            let position = self.code.position(page);
            let code = &self.code.pages[position];
            let base = page_base(page);

            #[cfg(feature = "jit")]
            let jit = self.jit.enabled();
            #[cfg(not(feature = "jit"))]
            let jit = false;
            let (table, page_budget) =
                self.weights.as_ref().map_or((&UNWEIGHED, SLOTS as u64), |weights| {
                    (weights.by_code(), weights.page_budget())
                });
            let instret = self.instret + if weighted { retired } else { budget - remaining };
            macro_rules! run {
                ($precise:literal, $jit:literal, false, $tracer:expr, $ctx:expr) => {
                    run!(@call execute::<M, _, _, $precise, $jit, false>, $tracer, $ctx)
                };
                ($precise:literal, $jit:literal, true, $tracer:expr, $ctx:expr) => {
                    run!(@call execute_weighted::<M, _, _, $precise, $jit>, $tracer, $ctx)
                };
                (@call $function:expr, $tracer:expr, $ctx:expr) => {
                    $function(
                        &mut self.registers,
                        #[cfg(feature = "a")]
                        &mut self.reservation,
                        memory,
                        code,
                        #[cfg(feature = "jit")]
                        &mut self.code.counters[position],
                        #[cfg(feature = "jit")]
                        self.jit.threshold(),
                        base,
                        slot,
                        &mut remaining,
                        $tracer,
                        $ctx,
                        instret,
                        table,
                        page_budget,
                        &mut retired,
                    )
                };
            }
            let (exit, pc) = if T::ENABLED {
                // A tracer sees every instruction, so the budget is checked before each of them
                // and nothing is compiled.
                if weighted {
                    run!(true, false, true, &mut *tracer, &mut *ctx)
                } else {
                    run!(true, false, false, &mut *tracer, &mut *ctx)
                }
            } else {
                match (remaining >= page_budget, jit, weighted) {
                    (true, false, false) => run!(false, false, false, &mut (), &mut ()),
                    (true, true, false) => run!(false, true, false, &mut (), &mut ()),
                    (false, false, false) => run!(true, false, false, &mut (), &mut ()),
                    (false, true, false) => run!(true, true, false, &mut (), &mut ()),
                    (true, false, true) => run!(false, false, true, &mut (), &mut ()),
                    (true, true, true) => run!(false, true, true, &mut (), &mut ()),
                    (false, false, true) => run!(true, false, true, &mut (), &mut ()),
                    (false, true, true) => run!(true, true, true, &mut (), &mut ()),
                }
            };
            self.pc = pc;

            match exit {
                Exit::Leave => {}
                Exit::Budget => break Stop::Budget,
                Exit::Decode => {
                    if let Err(error) = self.code.decode(memory, page, slot_of(pc)) {
                        break Stop::Memory(error);
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
                Exit::Memory(error) => break Stop::Memory(error),
                #[cfg(feature = "jit")]
                Exit::Hot => self.enter_region(
                    memory,
                    page,
                    position,
                    slot_of(pc),
                    &mut remaining,
                    &mut retired,
                ),
            }
        };

        self.spent += budget - remaining;
        self.instret += if weighted { retired } else { budget - remaining };
        stop
    }
}

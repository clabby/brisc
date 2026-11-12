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
    instret: u64,
    spent: u64,
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
    /// The largest budget that a run uses: a larger one is the same as this one. It is what fits
    /// in a signed 64-bit integer, which is more than any hart spends in practice.
    pub const MAX_BUDGET: u64 = i64::MAX as u64;

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

    /// Returns the number of instructions retired.
    pub const fn instret(&self) -> u64 {
        self.instret
    }

    /// Returns what the instructions retired have cost, and what the memory charged for their
    /// accesses, in the units of the budget of [`Cpu::run`]: the sum of the weights that applied
    /// as each instruction retired and of the prices that were paid. Without [`Weights`] and
    /// prices it is [`Cpu::instret`].
    ///
    /// The sum wraps around at the largest [`u64`]. What a run spent is the difference between its
    /// values before and after it, computed with wrapping arithmetic.
    pub const fn spent(&self) -> u64 {
        self.spent
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
    /// A budget above [`Cpu::MAX_BUDGET`] counts as that. An instruction costs one unit, or the
    /// weight that [`Cpu::set_weights`] gave to its opcode.
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
        let budget = budget.min(Self::MAX_BUDGET);
        let mut remaining = budget;
        // The instructions retired, when the budget does not tell: see [`execute`].
        let mut retired = 0;
        let weighted = self.weights.is_some();
        let counted = weighted || M::CHARGES;

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
            let instret = self.instret + if counted { retired } else { budget - remaining };
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

        self.spent = self.spent.wrapping_add(budget - remaining);
        self.instret += if counted { retired } else { budget - remaining };
        stop
    }
}

#[cfg(test)]
mod test {
    //! Checks that a memory that charges for accesses spends the budget exactly.

    use super::*;
    use crate::memory::{Memory, SparseMemory};
    use brisc_isa::{rv, rv_code, XWord};

    const CODE: XWord = 0x1000;
    const PRICE: u64 = 100;

    /// A memory in which each page that a store maps costs `PRICE` units.
    fn memory(code: &[u8], price: u64) -> impl Memory<Error = core::convert::Infallible> {
        let mut memory =
            SparseMemory::new().with_growth(move |before, after| price * (after - before) as u64);
        memory.write(CODE, code).unwrap();
        memory
    }

    #[test]
    fn a_store_that_maps_a_page_pays_for_it_before_it_runs() {
        let code = rv_code! {
            lui a1, 0x80;
            sw a0, 0(a1);
            sw a0, 4(a1);
            lui a2, 0x90;
            sw a0, 0(a2);
            ecall;
        };
        // What the first n instructions cost: the stores to pages that nothing has written to yet
        // cost a price on top of an instruction.
        let prefix = [0, 1, 2 + PRICE, 3 + PRICE, 4 + PRICE, 5 + 2 * PRICE, 6 + 2 * PRICE];
        for budget in 0..=prefix[6] + 3 {
            let mut memory = memory(&code, PRICE);
            let mut cpu = Cpu::new(CODE);

            let stop = cpu.run(&mut memory, budget);

            let retired = prefix.iter().rposition(|cost| *cost <= budget).unwrap();
            assert_eq!(cpu.instret, retired as u64, "budget {budget}");
            assert_eq!(cpu.spent, prefix[retired], "budget {budget}");
            // The pc stays on an ecall, which counts as retired.
            assert_eq!(cpu.pc, CODE + 4 * retired.min(5) as XWord, "budget {budget}");
            assert_eq!(
                stop,
                if retired == 6 { Stop::Ecall } else { Stop::Budget },
                "budget {budget}"
            );
            // A store that was not paid for has not mapped its page.
            let stores_that_ran = [retired >= 2, retired >= 5];
            let mut word = [0; 4];
            for (page, ran) in [(0x80000, stores_that_ran[0]), (0x90000, stores_that_ran[1])] {
                memory.read(page, &mut word).unwrap();
                assert_eq!(word, [0; 4]);
                assert_eq!(memory.access_cost(page, 4, true) == 0, ran, "budget {budget}");
            }
        }
    }

    #[test]
    fn a_charge_in_the_middle_of_a_page_does_not_let_the_hart_overrun_the_budget() {
        // After the store pays 2000 units, the budget no longer covers a run through the page,
        // which the hart checks once it has paid.
        let mut code = rv_code! {
            lui a1, 0x80;
            sw a0, 0(a1);
        }
        .to_vec();
        for _ in 0..300 {
            code.extend(rv!(addi a0, a0, 1).to_le_bytes());
        }
        code.extend(rv!(ecall).to_le_bytes());
        let mut memory = memory(&code, 2000);
        let mut cpu = Cpu::new(CODE);

        assert_eq!(cpu.run(&mut memory, 2200), Stop::Budget);

        // The instructions that the budget left over after the first two: 2200 - 1 - 2001.
        assert_eq!((cpu.instret, cpu.spent), (2 + 198, 2200));
        assert_eq!(cpu.registers.get(10), 198);
        assert_eq!(cpu.pc, CODE + 4 * 200);
    }

    #[test]
    #[cfg(feature = "a")]
    fn an_atomic_that_maps_a_page_pays_for_it_before_it_runs() {
        let code = rv_code! {
            lui a1, 0x80;
            amoadd.w a2, a0, (a1);
            ecall;
        };
        let mut memory = memory(&code, PRICE);
        let mut cpu = Cpu::new(CODE);
        cpu.registers.set(10, 5);

        // One unit short of the atomic and its price.
        assert_eq!(cpu.run(&mut memory, PRICE + 1), Stop::Budget);
        assert_eq!((cpu.instret, cpu.spent, cpu.pc), (1, 1, CODE + 4));
        assert_eq!(memory.access_cost(0x80000, 4, true), PRICE);

        assert_eq!(cpu.run(&mut memory, PRICE + 2), Stop::Ecall);
        assert_eq!((cpu.instret, cpu.spent), (3, 3 + PRICE));
        assert_eq!(memory.load::<4>(0x80000), Ok(5u32.to_le_bytes()));
    }

    #[test]
    #[cfg(feature = "a")]
    fn a_store_conditional_is_charged_whether_or_not_it_stores() {
        let code = rv_code! {
            lui a1, 0x80;
            sc.w a0, a2, (a1);
            ecall;
        };
        // Without a reservation the store-conditional fails and maps nothing. It is asked for like
        // any store, and costs the price on top of its weight.
        let mut memory = memory(&code, PRICE);
        let mut cpu = Cpu::new(CODE);
        assert_eq!(cpu.run(&mut memory, 1 + PRICE), Stop::Budget);
        assert_eq!((cpu.instret(), cpu.spent(), cpu.pc), (1, 1, CODE + 4));

        assert_eq!(cpu.run(&mut memory, 2 + PRICE), Stop::Ecall);
        assert_eq!((cpu.instret(), cpu.spent(), cpu.registers.get(10)), (3, 3 + PRICE, 1));
        // Nothing was stored, so the page is still unmapped.
        assert_eq!(memory.access_cost(0x80000, 4, true), PRICE);
    }

    #[test]
    fn the_total_spent_wraps_around_and_a_run_spends_the_difference() {
        let code = rv_code! {
            sw a0, 0(a1);
            ecall;
        };
        // A store and an ecall cost the largest budget between them.
        let price = Cpu::MAX_BUDGET - 2;
        let mut memory = memory(&code, price);
        let mut cpu = Cpu::new(CODE);
        let mut total = 0u64;
        for page in 0..3 {
            cpu.pc = CODE;
            cpu.registers.set(11, 0x10_0000 + 0x1000 * page);
            let before = cpu.spent();

            assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Ecall);

            assert_eq!(cpu.spent().wrapping_sub(before), Cpu::MAX_BUDGET, "page {page}");
            total = total.wrapping_add(Cpu::MAX_BUDGET);
        }
        assert_eq!((cpu.instret(), cpu.spent()), (6, total));
    }

    #[test]
    fn a_budget_above_the_maximum_counts_as_the_maximum() {
        let code = rv_code! {
            sw a0, 0(a1);
            ecall;
        };
        // The store costs more than the maximum budget, however large the budget that is asked for.
        let mut memory = memory(&code, Cpu::MAX_BUDGET);
        let mut cpu = Cpu::new(CODE);
        cpu.registers.set(11, 0x10_0000);

        assert_eq!(cpu.run(&mut memory, u64::MAX), Stop::Budget);
        assert_eq!((cpu.instret(), cpu.spent(), cpu.pc), (0, 0, CODE));
    }
}

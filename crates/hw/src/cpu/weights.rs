//! What instructions count for against a budget.

use super::op::Op;
use alloc::{boxed::Box, sync::Arc};
use brisc_isa::Opcode;

/// How much each opcode counts for against the budget of [`Cpu::run`](super::Cpu::run).
///
/// A budget is a number of units. Without weights, every instruction costs one unit, so the budget
/// is a number of instructions. With weights, an instruction costs the weight of its opcode, and
/// the hart stops with [`Stop::Budget`](super::Stop::Budget) when the next instruction costs more
/// than what is left. The weights say nothing about what the units mean: they may stand for
/// cycles, energy, or a price.
///
/// Compiled code spends the budget exactly as the interpreter does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Weights {
    /// The weight of each opcode, at the opcode's index in [`Opcode::ALL`].
    by_opcode: Arc<[u32]>,
    /// The weight of each [`Op::code`]. Codes that are not instructions weigh nothing.
    by_code: Box<[u32; 256]>,
    /// The largest weight.
    max: u32,
}

impl Weights {
    /// Weighs each opcode with `weight`. A weight of zero counts as one, so that every instruction
    /// spends something and a budget always ends a run.
    pub fn new(weight: impl Fn(Opcode) -> u32) -> Self {
        let by_opcode: Arc<[u32]> =
            Opcode::ALL.iter().map(|opcode| weight(*opcode).max(1)).collect();
        let mut by_code = Box::new([0; 256]);
        for (opcode, weight) in Opcode::ALL.iter().zip(by_opcode.iter()) {
            by_code[Op::code_for(*opcode, 2) as usize] = *weight;
            by_code[Op::code_for(*opcode, 4) as usize] = *weight;
        }
        let max = by_opcode.iter().copied().max().unwrap_or(1);
        Self { by_opcode, by_code, max }
    }

    /// Returns the weight of an opcode.
    pub fn weight(&self, opcode: Opcode) -> u32 {
        self.by_opcode[opcode as usize]
    }

    /// The weights by [`Opcode`] as a number, for the JIT compiler.
    #[cfg(feature = "jit")]
    pub(super) const fn by_opcode(&self) -> &Arc<[u32]> {
        &self.by_opcode
    }

    /// The weights by [`Op::code`], for the interpreter.
    pub(super) fn by_code(&self) -> &[u32; 256] {
        &self.by_code
    }

    /// The budget that covers a run through a whole page, however the instructions of the page
    /// are weighed: every instruction advances by at least one slot.
    pub(super) const fn page_budget(&self) -> u64 {
        super::SLOTS as u64 * self.max as u64
    }
}

/// The weights of the interpreter that does not weigh: unused, because it counts every
/// instruction as one.
pub(super) static UNWEIGHED: [u32; 256] = [0; 256];

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        cpu::{Cpu, Stop},
        memory::{Memory, SparseMemory},
    };
    use brisc_isa::{rv_code, XWord};

    #[test]
    fn an_instruction_runs_only_if_the_budget_covers_its_weight() {
        let weights = Weights::new(|opcode| match opcode {
            Opcode::Addi => 3,
            Opcode::Slli => 5,
            // A weight of zero counts as one.
            _ => 0,
        });
        assert_eq!(weights.weight(Opcode::Ecall), 1);

        let code = rv_code! {
            addi a0, zero, 3;
            slli a0, a0, 2;
            addi a0, a0, 1;
            ecall;
        };
        // What the first n instructions cost, by n.
        let prefix = [0, 3, 8, 11, 12];
        for budget in 0..=14 {
            let mut memory = SparseMemory::new();
            let Ok(()) = memory.write(0x1000, &code);
            let mut cpu = Cpu::new(0x1000);
            cpu.set_weights(Some(weights.clone()));

            let stop = cpu.run(&mut memory, budget);

            let retired = prefix.iter().rposition(|cost| *cost <= budget).unwrap();
            assert_eq!(cpu.instret, retired as u64, "budget {budget}");
            assert_eq!(cpu.spent, prefix[retired], "budget {budget}");
            // The pc stays on an ecall, which counts as retired.
            assert_eq!(cpu.pc, 0x1000 + 4 * retired.min(3) as XWord, "budget {budget}");
            assert_eq!(stop, if retired == 4 { Stop::Ecall } else { Stop::Budget });
        }
    }

    #[test]
    fn without_weights_spending_is_retiring() {
        let code = rv_code! {
            addi a0, zero, 3;
            slli a0, a0, 2;
            ecall;
        };
        let mut memory = SparseMemory::new();
        let Ok(()) = memory.write(0x1000, &code);
        let mut cpu = Cpu::new(0x1000);
        assert_eq!(cpu.run(&mut memory, 2), Stop::Budget);
        assert_eq!((cpu.instret, cpu.spent), (2, 2));
    }
}

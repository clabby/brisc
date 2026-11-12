//! Tracing of the instructions that a hart executes.

use super::Registers;
use crate::memory::Memory;
use brisc_isa::{Instruction, XWord};

/// An instruction that a hart is about to execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    /// The address of the instruction.
    pub pc: XWord,
    /// The instruction. Its length says how far the program counter moves if the instruction does
    /// not jump.
    pub instruction: Instruction,
    /// The number of instructions that the hart has retired before this one, which is also its
    /// index in the run, counting from zero.
    pub instret: u64,
}

/// Observes every instruction that a hart executes, before it executes it.
///
/// A [`Tracer`] is given what the instruction is about to read: the registers and the memory as
/// they are before it executes, with the [`Step`] that says which instruction it is. The address of
/// a load or a store is the sum of a register and the immediate of the instruction, for example.
/// The result of an instruction is what the next call, or the end of the run, finds in the
/// registers. An instruction that stops the hart without retiring, such as an `ebreak`, is traced
/// before it stops, and traced again if it is run again.
///
/// `C` is a context that the hart passes through, without looking at it, and that the tracer may
/// change: for example the state of an emulator.
///
/// A hart that has a tracer executes the instructions one by one, as the interpreter does. It
/// does not compile them, whatever it is configured to do.
///
/// The hart does not trace at all, at no cost, with the tracer that does nothing, which is `()`.
pub trait Tracer<M: Memory, C> {
    /// Whether the hart calls the tracer. A tracer that sets this to `false` is never called, and
    /// the hart is as fast as without one.
    const ENABLED: bool = true;

    /// Called with the instruction that the hart is about to execute.
    fn trace(&mut self, ctx: &mut C, memory: &M, registers: &Registers, step: &Step);
}

/// Tracers that run one after the other, in the order of the tuple. A tuple traces if any of its
/// tracers does, and only the tracers that do are called.
macro_rules! tuple_tracers {
    ($(($($index:tt $name:ident),+))+) => {$(
        impl<M: Memory, C, $($name: Tracer<M, C>),+> Tracer<M, C> for ($($name,)+) {
            const ENABLED: bool = $($name::ENABLED)||+;

            fn trace(&mut self, ctx: &mut C, memory: &M, registers: &Registers, step: &Step) {
                $(
                    if $name::ENABLED {
                        self.$index.trace(ctx, memory, registers, step);
                    }
                )+
            }
        }
    )+};
}

tuple_tracers! {
    (0 T0)
    (0 T0, 1 T1)
    (0 T0, 1 T1, 2 T2)
    (0 T0, 1 T1, 2 T2, 3 T3)
    (0 T0, 1 T1, 2 T2, 3 T3, 4 T4)
    (0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5)
    (0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6)
    (0 T0, 1 T1, 2 T2, 3 T3, 4 T4, 5 T5, 6 T6, 7 T7)
}

/// The tracer that does nothing, and costs nothing.
impl<M: Memory, C> Tracer<M, C> for () {
    const ENABLED: bool = false;

    fn trace(&mut self, _: &mut C, _: &M, _: &Registers, _: &Step) {}
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        cpu::{Cpu, Stop},
        memory::SparseMemory,
        REG_A0,
    };
    use brisc_isa::{rv_code, Opcode};

    /// Records what it is shown, with the value that a0 has at the time.
    #[derive(Default)]
    struct Recorder(Vec<(Step, XWord)>);

    impl Tracer<SparseMemory, u32> for Recorder {
        fn trace(&mut self, ctx: &mut u32, _: &SparseMemory, registers: &Registers, step: &Step) {
            *ctx += 1;
            self.0.push((*step, registers.get(REG_A0)));
        }
    }

    #[test]
    fn every_instruction_is_traced_before_it_executes() {
        let mut memory = SparseMemory::new();
        let code = rv_code! {
            addi a0, zero, 3;
            slli a0, a0, 2;
            ebreak;
            ecall;
        };
        let Ok(()) = memory.write(0x1000, &code);
        let mut cpu = Cpu::new(0x1000);
        let (mut recorder, mut count) = (Recorder::default(), 0);

        // The budget runs out after the first instruction, and the second run continues.
        assert_eq!(cpu.run_traced(&mut memory, 1, &mut recorder, &mut count), Stop::Budget);
        assert_eq!(cpu.run_traced(&mut memory, 5, &mut recorder, &mut count), Stop::Ebreak);

        let steps: Vec<_> = recorder
            .0
            .iter()
            .map(|(step, a0)| (step.pc, step.instruction.opcode, step.instret, *a0))
            .collect();
        assert_eq!(
            steps,
            [
                (0x1000, Opcode::Addi, 0, 0),
                (0x1004, Opcode::Slli, 1, 3),
                (0x1008, Opcode::Ebreak, 2, 12),
            ]
        );
        assert_eq!(count, 3);

        // An instruction that does not retire is traced again when it is run again.
        cpu.run_traced(&mut memory, 5, &mut recorder, &mut count);
        assert_eq!(recorder.0.len(), 4);
        assert_eq!(recorder.0[3].0.instret, 2);
    }

    #[test]
    fn a_tuple_of_tracers_shows_every_instruction_to_each() {
        let mut memory = SparseMemory::new();
        let code = rv_code! {
            addi a0, zero, 3;
            slli a0, a0, 2;
            ecall;
        };
        let Ok(()) = memory.write(0x1000, &code);
        let mut cpu = Cpu::new(0x1000);
        let mut tracers = (Recorder::default(), (), Recorder::default());
        let mut count = 0;

        assert_eq!(cpu.run_traced(&mut memory, u64::MAX, &mut tracers, &mut count), Stop::Ecall);
        assert_eq!(tracers.0 .0.len(), 3);
        assert_eq!(tracers.0 .0, tracers.2 .0);
        assert_eq!(count, 6);

        // A tuple traces if one of its tracers does.
        const { assert!(<(Recorder, ()) as Tracer<SparseMemory, u32>>::ENABLED) };
        const { assert!(!<((), ()) as Tracer<SparseMemory, u32>>::ENABLED) };
    }

    #[test]
    fn a_tracer_that_does_not_trace_is_not_called_from_a_tuple() {
        struct Disabled;

        impl Tracer<SparseMemory, u32> for Disabled {
            const ENABLED: bool = false;

            fn trace(&mut self, _: &mut u32, _: &SparseMemory, _: &Registers, _: &Step) {
                panic!("a tracer that does not trace was called");
            }
        }

        let mut memory = SparseMemory::new();
        let Ok(()) = memory.write(0x1000, &rv_code! { addi a0, zero, 3; ecall });
        let mut cpu = Cpu::new(0x1000);
        let mut tracers = (Recorder::default(), Disabled);
        let mut count = 0;

        assert_eq!(cpu.run_traced(&mut memory, u64::MAX, &mut tracers, &mut count), Stop::Ecall);
        assert_eq!(tracers.0 .0.len(), 2);
    }

    #[test]
    fn tracing_does_not_change_what_a_hart_does() {
        let code = rv_code! {
            addi a0, zero, 0;
            addi a1, zero, 300;
            addi a0, a0, 1;
            bne a0, a1, -4;
            ecall;
        };
        let run = |traced: bool| {
            let mut memory = SparseMemory::new();
            let Ok(()) = memory.write(0x1000, &code);
            let mut cpu = Cpu::new(0x1000);
            let stop = if traced {
                cpu.run_traced(&mut memory, u64::MAX, &mut Recorder::default(), &mut 0)
            } else {
                cpu.run(&mut memory, u64::MAX)
            };
            (stop, cpu.pc, cpu.instret, cpu.registers.get(REG_A0))
        };
        assert_eq!(run(true), run(false));
    }
}

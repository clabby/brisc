//! Pipeline registers and control signals.

use crate::memory::Address;
use bitflags::bitflags;
use brisc_isa::{Instruction, Word, XWord};

bitflags! {
    /// Represents control signals given to an instruction during its traversal through the pipeline.
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ControlSignals: u8 {
        /// Memory was read.
        const MEM_READ = 0b0000_0001;
        /// Memory was written to.
        const MEM_WRITE = 0b0000_0010;
        /// A register was written to.
        const REG_WRITE = 0b0000_0100;
        /// Memory was read into a register.
        const MEM_TO_REG = Self::MEM_READ.bits() | Self::REG_WRITE.bits();
        /// A branch was taken.
        const BRANCH = 0b0000_1000;
        /// A jump was taken.
        const JUMP = 0b0001_0000;
    }
}

/// The [PipelineRegister] represents an intermediate state of an instruction's execution within
/// the CPU pipeline. As the [PipelineRegister] passes through each stage, the type is saturated.
/// Ultimately, it is discarded after it has made its way through the register write-back stage
/// via [PipelineRegister::advance].
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineRegister {
    /// The current program counter.
    pub pc: XWord,
    /// The register file.
    pub registers: [XWord; 32],
    /// The next program counter.
    pub next_pc: XWord,
    /// The current instruction (raw).
    pub instruction_raw: Option<Word>,
    /// The current instruction (decoded).
    pub instruction: Option<Instruction>,
    /// The cached value of the `rs1` register.
    pub rs1_value: Option<XWord>,
    /// The cached value of the `rs2` register.
    pub rs2_value: Option<XWord>,
    /// The cached sign-extended immediate.
    pub immediate: Option<XWord>,
    /// The cached `rd` register index.
    pub rd: Option<u8>,
    /// The control signals given to the instruction during its execution.
    pub control_signals: ControlSignals,
    /// The result of the ALU computation, if one occurred.
    pub alu_result: Option<XWord>,
    /// The data read from memory, if any.
    pub memory: Option<XWord>,
    /// If the last instruction exited the program, this will be `true`.
    pub exit: bool,
    /// The exit code of the program, if it exited.
    pub exit_code: XWord,
}

impl PipelineRegister {
    /// Creates a new [PipelineRegister] with the given program counter.
    pub fn new(pc: XWord) -> Self {
        Self { pc, ..Default::default() }
    }

    /// Clear the [PipelineRegister] and set the program counter to the next program counter.
    pub fn advance(&mut self) {
        *self = Self { pc: self.next_pc, registers: self.registers, ..Default::default() };
    }

    /// Computes the effective address of the memory operation if [Self::rs1_value] and
    /// [Self::immediate] are [Some].
    pub fn effective_address(&self) -> Option<Address> {
        self.rs1_value.and_then(|rs1| self.immediate.map(|imm| rs1 + imm))
    }
}

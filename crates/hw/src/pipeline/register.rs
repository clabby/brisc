//! Pipeline registers and control signals.

use crate::memory::Address;
use brisc_isa::{Instruction, Word, XWord};

/// The [PipelineRegister] represents an intermediate state of an instruction's execution within
/// the CPU pipeline. As the [PipelineRegister] passes through each stage, the type is saturated.
/// Ultimately, it is discarded after it has made its way through the register write-back stage
/// via [PipelineRegister::advance].
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineRegister {
    /// If the last instruction exited the program, this will be `true`.
    pub exit: bool,
    /// The exit code of the program, if it exited.
    pub exit_code: XWord,
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
    /// The result of the ALU computation, if one occurred.
    pub alu_result: Option<XWord>,
    /// The data read from memory, if any.
    pub memory: Option<XWord>,
    /// The load reservation address, if any.
    #[cfg(feature = "a")]
    pub reservation: Option<Address>,
}

impl PipelineRegister {
    /// Creates a new [PipelineRegister] with the given program counter.
    pub fn new(pc: XWord) -> Self {
        Self { pc, ..Default::default() }
    }

    /// Clear the [PipelineRegister] and set the program counter to the next program counter.
    pub fn advance(&mut self) {
        *self = Self {
            pc: self.next_pc,
            registers: self.registers,
            #[cfg(feature = "a")]
            reservation: self.reservation,
            ..Default::default()
        };
    }

    /// Computes the effective address of the memory operation if [Self::rs1_value] and
    /// [Self::immediate] are [Some].
    pub fn effective_address(&self) -> Option<Address> {
        self.rs1_value.and_then(|rs1| self.immediate.map(|imm| rs1.wrapping_add(imm)))
    }
}

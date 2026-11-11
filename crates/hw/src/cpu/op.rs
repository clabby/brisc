//! Decoded instructions.

use super::{Fault, Registers};
use brisc_isa::{Instruction, Opcode, Word};

/// An instruction in the form the interpreter executes: an [Instruction] packed into eight
/// bytes.
///
/// The length of the instruction is part of [Op::code], so that every arm of the interpreter's
/// dispatch knows statically how far to advance and the address of the next [Op] never depends
/// on the contents of the current one.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(8))]
pub(super) struct Op {
    /// What the interpreter does: one of the constants in [code], or the [Op::code] of an
    /// [Opcode] and a length.
    pub(super) code: u8,
    /// The destination register, with `x0` replaced by [Registers::SINK].
    pub(super) rd: u8,
    /// The first source register.
    pub(super) rs1: u8,
    /// The second source register.
    pub(super) rs2: u8,
    /// The immediate, as in [Instruction::imm].
    pub(super) imm: i32,
}

// Eight bytes with eight-byte alignment: a slot never straddles two cache lines.
const _: () = assert!(size_of::<Op>() == 8 && align_of::<Op>() == 8);

/// The values of [Op::code] that do not stand for an instruction.
pub(super) mod code {
    /// The initial content of a slot: leaves the interpreter to have the slot decoded. It is
    /// zero so that fresh slots are zeroed memory.
    pub(in super::super) const DECODE: u8 = 0;

    /// Fills the slots past the last instruction of a page: leaves the interpreter to continue
    /// on the next page.
    pub(in super::super) const PAGE_END: u8 = 1;

    /// Stands in for a word that is not an instruction the hart can execute.
    pub(in super::super) const ILLEGAL: u8 = 2;

    /// The code of the first [Opcode](brisc_isa::Opcode). The others follow in the order of
    /// their discriminants.
    pub(in super::super) const FIRST_OPCODE: u8 = 3;

    /// The bit that is set in the code of an instruction that is four bytes long and clear in
    /// the code of one that is two bytes long.
    pub(in super::super) const WIDE: u8 = 0x80;
}

// The codes of the opcodes fit below the bit that holds the length.
const _: () = assert!(code::FIRST_OPCODE as usize + Opcode::ALL.len() <= code::WIDE as usize);

impl Op {
    /// A slot that has not been decoded.
    pub(super) const DECODE: Self = Self::bare(code::DECODE);

    /// A slot past the last instruction of a page.
    pub(super) const PAGE_END: Self = Self::bare(code::PAGE_END);

    /// A slot whose word is not an instruction the hart can execute.
    pub(super) const ILLEGAL: Self = Self::bare(code::ILLEGAL);

    const fn bare(code: u8) -> Self {
        Self { code, rd: 0, rs1: 0, rs2: 0, imm: 0 }
    }

    /// Returns the [Op::code] of an instruction with this opcode that is `length` bytes long.
    pub(super) const fn code(opcode: Opcode, length: u8) -> u8 {
        let wide = if length == 4 { code::WIDE } else { 0 };
        (code::FIRST_OPCODE + opcode as u8) | wide
    }

    /// Packs `instruction`.
    pub(super) const fn new(instruction: Instruction) -> Self {
        Self {
            code: Self::code(instruction.opcode, instruction.length),
            rd: if instruction.rd == 0 { Registers::SINK } else { instruction.rd },
            rs1: instruction.rs1,
            rs2: instruction.rs2,
            imm: instruction.imm,
        }
    }

    /// Returns the opcode of the instruction.
    ///
    /// ## Panics
    ///
    /// Panics if the [Op] does not stand for an instruction.
    #[cfg(feature = "a")]
    pub(super) const fn opcode(self) -> Opcode {
        Opcode::ALL[((self.code & !code::WIDE) - code::FIRST_OPCODE) as usize]
    }
}

/// Decodes the instruction in the low bits of `word`.
pub(super) const fn decode(word: Word) -> Result<Instruction, Fault> {
    match Instruction::decode(word) {
        Ok(instruction) => Ok(instruction),
        Err(error) => Err(Fault::Decode(error)),
    }
}

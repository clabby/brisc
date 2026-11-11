//! Decoded instructions.

use super::{Fault, Registers};
use brisc_isa::{Instruction, Opcode, Word};

/// An instruction in the form the interpreter executes: an [Instruction] packed into eight
/// bytes.
///
/// The length of the instruction is part of [Op::code], so that every arm of the interpreter's
/// dispatch knows statically how far to advance and the address of the next [Op] never depends
/// on the contents of the current one.
///
/// The fields share one 64-bit word, which the interpreter loads once and takes apart with
/// shifts: the code in bits 0 to 7, the destination register in bits 8 to 15, the first source
/// register in bits 16 to 23, the second in bits 24 to 31 and the immediate in bits 32 to 63.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(8))]
pub(super) struct Op(u64);

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

    const fn bare(code: u8) -> Self {
        Self::pack(code, 0, 0, 0, 0)
    }

    const fn pack(code: u8, rd: u8, rs1: u8, rs2: u8, imm: i32) -> Self {
        Self(
            code as u64 |
                (rd as u64) << 8 |
                (rs1 as u64) << 16 |
                (rs2 as u64) << 24 |
                (imm as u32 as u64) << 32,
        )
    }

    /// A slot whose `word` is not an instruction the hart can execute. The word is kept in the
    /// immediate, for [decode] to say what is wrong with it.
    pub(super) const fn illegal(word: Word) -> Self {
        Self::pack(code::ILLEGAL, 0, 0, 0, word as i32)
    }

    /// Returns the [Op::code] of an instruction with this opcode that is `length` bytes long.
    pub(super) const fn code_for(opcode: Opcode, length: u8) -> u8 {
        let wide = if length == 4 { code::WIDE } else { 0 };
        (code::FIRST_OPCODE + opcode as u8) | wide
    }

    /// Packs `instruction`.
    pub(super) const fn new(instruction: Instruction) -> Self {
        Self::pack(
            Self::code_for(instruction.opcode, instruction.length),
            if instruction.rd == 0 { Registers::SINK } else { instruction.rd },
            instruction.rs1,
            instruction.rs2,
            instruction.imm,
        )
    }

    /// Returns what the interpreter does: one of the constants in [code], or the code of an
    /// [Opcode] and a length.
    #[inline(always)]
    pub(super) const fn code(self) -> u8 {
        self.0 as u8
    }

    /// Returns the position in the register file of the destination register.
    #[inline(always)]
    pub(super) const fn rd(self) -> usize {
        (self.0 >> 8) as usize & (Registers::LEN - 1)
    }

    /// Returns the position in the register file of the first source register.
    #[inline(always)]
    pub(super) const fn rs1(self) -> usize {
        (self.0 >> 16) as usize & (Registers::LEN - 1)
    }

    /// Returns the position in the register file of the second source register.
    #[inline(always)]
    pub(super) const fn rs2(self) -> usize {
        (self.0 >> 24) as usize & (Registers::LEN - 1)
    }

    /// Returns the immediate, as in [Instruction::imm].
    #[inline(always)]
    pub(super) const fn imm(self) -> i32 {
        (self.0 >> 32) as i32
    }

    /// Returns the opcode of the instruction.
    ///
    /// ## Panics
    ///
    /// Panics if the [Op] does not stand for an instruction.
    #[cfg(feature = "a")]
    pub(super) const fn opcode(self) -> Opcode {
        Opcode::ALL[((self.code() & !code::WIDE) - code::FIRST_OPCODE) as usize]
    }
}

/// Decodes the instruction in the low bits of `word` and checks that the hart can execute it.
#[inline(always)]
pub(super) const fn decode(word: Word) -> Result<Instruction, Fault> {
    let instruction = match Instruction::decode(word) {
        Ok(instruction) => instruction,
        Err(error) => return Err(Fault::Decode(error)),
    };

    if matches!(
        instruction.opcode,
        Opcode::Csrrw |
            Opcode::Csrrs |
            Opcode::Csrrc |
            Opcode::Csrrwi |
            Opcode::Csrrsi |
            Opcode::Csrrci
    ) {
        return check_csr(instruction);
    }
    Ok(instruction)
}

/// Checks that an instruction that accesses a control and status register does not write to
/// one that is read-only.
#[cold]
const fn check_csr(instruction: Instruction) -> Result<Instruction, Fault> {
    // The instructions that set or clear bits only write if they name a source other than `x0`
    // or an immediate other than zero.
    let writes = match instruction.opcode {
        Opcode::Csrrs | Opcode::Csrrc | Opcode::Csrrsi | Opcode::Csrrci => instruction.rs1 != 0,
        _ => true,
    };
    // The two highest bits of the address of a read-only register are set.
    if writes && instruction.imm >> 10 == 0b11 {
        return Err(Fault::ReadOnlyCsr(instruction.imm as u16));
    }
    Ok(instruction)
}

//! Compressed instructions, from the RISC-V `c` extension.

use crate::{
    bits, twiddle, HalfWord, Instruction, InstructionDecodeError, Opcode, REG_RA, REG_SP, REG_ZERO,
};

mod types;
pub use types::*;

/// C_REGISTER_OFFSET is the offset of the register mapping from the `C` instructions to regular
/// 32 bit instructions. In the `C` extension, register fields are only allotted 3 bits,
/// allowing for 8 possible register designations.
pub const C_REG_OFFSET: u8 = 8;

/// Maps a compressed register index to a regular register index, by adding [`C_REG_OFFSET`].
/// ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┐
/// │ 000 │ 001 │ 010 │ 011 │ 100 │ 101 │ 110 │ 111 │
/// ├─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┤
/// │ x8  │ x9  │ x10 │ x11 │ x12 │ x13 │ x14 │ x15 │
/// └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┘
pub const fn map_compressed_reg_idx(reg: u8) -> u8 {
    reg + C_REG_OFFSET
}

/// Returns the value of a `Result` or returns its error from the enclosing function.
macro_rules! tri {
    ($result:expr) => {
        match $result {
            Ok(value) => value,
            Err(error) => return Err(error),
        }
    };
}

/// Builds the [`Instruction`] that a 16-bit instruction expands to.
const fn expansion(opcode: Opcode, rd: u8, rs1: u8, rs2: u8, imm: i32) -> Instruction {
    Instruction { opcode, rd, rs1, rs2, imm, length: 2 }
}

/// Sign-extends the low `bits` bits of `value`.
const fn sext(value: i32, bits: u32) -> i32 {
    value << (32 - bits) >> (32 - bits)
}

const fn invalid_function(q_a: u8, q_b: u8) -> InstructionDecodeError {
    InstructionDecodeError::InvalidFunction { q_a, q_b }
}

/// A compressed RISC-V instruction, with variants for each compressed opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum CompressedInstruction {
    /// C0 quadrant instruction.
    C0(C0) = 0b00,
    /// C1 quadrant instruction.
    C1(C1) = 0b01,
    /// C2 quadrant instruction.
    C2(C2) = 0b10,
}

impl CompressedInstruction {
    /// Decodes a [`CompressedInstruction`] from a 16-bit [`HalfWord`].
    pub const fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let opcode = instruction & 0b11;
        match opcode {
            0b00 => Ok(Self::C0(tri!(C0::decode(instruction)))),
            0b01 => Ok(Self::C1(tri!(C1::decode(instruction)))),
            0b10 => Ok(Self::C2(tri!(C2::decode(instruction)))),
            _ => Err(InstructionDecodeError::InvalidOpcode(opcode as u8)),
        }
    }

    /// Maps the [`CompressedInstruction`] to the [`Instruction`] it abbreviates.
    pub const fn expand(self) -> Instruction {
        match self {
            Self::C0(c0) => c0.expand(),
            Self::C1(c1) => c1.expand(),
            Self::C2(c2) => c2.expand(),
        }
    }
}

/// A RISC-V C0 instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C0 {
    /// C.ADDI4SPN instruction.
    CAddi4spn(CIWType),
    /// C.LW instruction.
    CLw(CLType),
    /// C.SW instruction.
    CSw(CSType),
    /// C.LD instruction.
    #[cfg(feature = "64-bit")]
    CLd(CLType),
    /// C.SD instruction.
    #[cfg(feature = "64-bit")]
    CSd(CSType),
}

impl C0 {
    /// Decodes a [`C0`] instruction from a 16-bit [`HalfWord`].
    pub const fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct3 = bits!(u8, instruction, 13..16);
        match funct3 {
            // A zero immediate is reserved, which makes the all-zero halfword illegal.
            0b000 => match CIWType::decode(instruction) {
                CIWType { imm: 0, .. } => Err(InstructionDecodeError::Reserved),
                ciw => Ok(Self::CAddi4spn(ciw)),
            },
            0b010 => Ok(Self::CLw(CLType::decode(instruction))),
            0b110 => Ok(Self::CSw(CSType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b011 => Ok(Self::CLd(CLType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b111 => Ok(Self::CSd(CSType::decode(instruction))),
            _ => Err(invalid_function(funct3, 0)),
        }
    }

    /// Maps the compressed instruction to the [`Instruction`] it abbreviates.
    pub const fn expand(self) -> Instruction {
        match self {
            // C.ADDI4SPN expands to `addi rd', x2, nzuimm[9:2]`
            Self::CAddi4spn(ciw) => expansion(
                Opcode::Addi,
                map_compressed_reg_idx(ciw.rd),
                REG_SP as u8,
                0,
                twiddle!(i32, ciw.imm, 2..6, 6..8, 0..1, 1..2) << 2,
            ),
            // C.LW expands to `lw rd', offset[6:2](rs1')`
            Self::CLw(cl) => expansion(
                Opcode::Lw,
                map_compressed_reg_idx(cl.rd),
                map_compressed_reg_idx(cl.rs1),
                0,
                twiddle!(i32, cl.imm, 0..1, 2..5, 1..2) << 2,
            ),
            // C.SW expands to `sw rs2', offset[6:2](rs1')`
            Self::CSw(cs) => expansion(
                Opcode::Sw,
                0,
                map_compressed_reg_idx(cs.rs1),
                map_compressed_reg_idx(cs.rs2),
                twiddle!(i32, cs.imm, 0..1, 2..5, 1..2) << 2,
            ),
            // C.LD expands to `ld rd', offset[7:3](rs1')`
            #[cfg(feature = "64-bit")]
            Self::CLd(cl) => expansion(
                Opcode::Ld,
                map_compressed_reg_idx(cl.rd),
                map_compressed_reg_idx(cl.rs1),
                0,
                twiddle!(i32, cl.imm, 0..2, 2..5) << 3,
            ),
            // C.SD expands to `sd rs2', offset[7:3](rs1')`
            #[cfg(feature = "64-bit")]
            Self::CSd(cs) => expansion(
                Opcode::Sd,
                0,
                map_compressed_reg_idx(cs.rs1),
                map_compressed_reg_idx(cs.rs2),
                twiddle!(i32, cs.imm, 0..2, 2..5) << 3,
            ),
        }
    }
}

/// A RISC-V C1 instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C1 {
    /// C.NOP / C.ADDI instruction.
    CAddi(CIType),
    /// C.JAL instruction.
    CJal(CJType),
    /// C.LI instruction.
    CLi(CIType),
    /// C.ADDI16SP instruction.
    CAddi16sp(CIType),
    /// C.LUI instruction.
    CLui(CIType),
    /// C.SRLI, C.SRAI, C.ANDI, C.SUB, C.XOR, C.OR, C.AND, C.SUBW, C.ADDW instructions.
    SubFunct(C1SubFunct),
    /// C.J instruction.
    CJ(CJType),
    /// C.BEQZ instruction.
    CBeqz(CBType),
    /// C.BNEZ instruction.
    CBnez(CBType),
    /// C.ADDIW instruction.
    #[cfg(feature = "64-bit")]
    CAddiw(CIType),
}

impl C1 {
    /// Decodes a [`C1`] instruction from a 16-bit [`HalfWord`].
    pub const fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct3 = bits!(u8, instruction, 13..16);
        let ci = CIType::decode(instruction);

        match funct3 {
            0b000 => Ok(Self::CAddi(ci)),
            // The encoding of C.JAL on a 32-bit architecture is C.ADDIW on a 64-bit one, where
            // it needs a destination other than `x0`.
            #[cfg(not(feature = "64-bit"))]
            0b001 => Ok(Self::CJal(CJType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b001 if ci.rs1_rd == 0 => Err(InstructionDecodeError::Reserved),
            #[cfg(feature = "64-bit")]
            0b001 => Ok(Self::CAddiw(ci)),
            0b010 => Ok(Self::CLi(ci)),
            // C.ADDI16SP and C.LUI need an immediate other than zero.
            0b011 if ci.imm == 0 => Err(InstructionDecodeError::Reserved),
            0b011 if ci.rs1_rd == REG_SP as u8 => Ok(Self::CAddi16sp(ci)),
            0b011 => Ok(Self::CLui(ci)),
            0b100 => Ok(Self::SubFunct(tri!(C1SubFunct::decode(instruction)))),
            0b101 => Ok(Self::CJ(CJType::decode(instruction))),
            0b110 => Ok(Self::CBeqz(CBType::decode(instruction))),
            0b111 => Ok(Self::CBnez(CBType::decode(instruction))),
            _ => Err(invalid_function(funct3, 0)),
        }
    }

    /// Maps the compressed instruction to the [`Instruction`] it abbreviates.
    pub const fn expand(self) -> Instruction {
        match self {
            // C.ADDI expands to `addi rd, rd, imm[5:0]`
            Self::CAddi(ci) => {
                expansion(Opcode::Addi, ci.rs1_rd, ci.rs1_rd, 0, sext(ci.imm as i32, 6))
            }
            // C.JAL expands to `jal x1, offset[11:1]`
            Self::CJal(cj) => expansion(Opcode::Jal, REG_RA as u8, 0, 0, jump_offset(cj)),
            // C.LI expands to `addi rd, x0, imm[5:0]`
            Self::CLi(ci) => {
                expansion(Opcode::Addi, ci.rs1_rd, REG_ZERO as u8, 0, sext(ci.imm as i32, 6))
            }
            // C.ADDI16SP expands to `addi x2, x2, nzimm[9:4]`
            Self::CAddi16sp(ci) => {
                let nzimm = twiddle!(i32, ci.imm, 5..6, 1..3, 3..4, 0..1, 4..5) << 4;
                expansion(Opcode::Addi, REG_SP as u8, REG_SP as u8, 0, sext(nzimm, 10))
            }
            // C.LUI expands to `lui rd, nzimm[17:12]`
            Self::CLui(ci) => {
                expansion(Opcode::Lui, ci.rs1_rd, 0, 0, sext((ci.imm as i32) << 12, 18))
            }
            Self::SubFunct(sub_funct) => sub_funct.expand(),
            // C.J expands to `jal x0, offset[11:1]`
            Self::CJ(cj) => expansion(Opcode::Jal, REG_ZERO as u8, 0, 0, jump_offset(cj)),
            // C.BEQZ expands to `beq rs1', x0, offset[8:1]`
            Self::CBeqz(cb) => expansion(
                Opcode::Beq,
                0,
                map_compressed_reg_idx(cb.rs1),
                REG_ZERO as u8,
                branch_offset(cb),
            ),
            // C.BNEZ expands to `bne rs1', x0, offset[8:1]`
            Self::CBnez(cb) => expansion(
                Opcode::Bne,
                0,
                map_compressed_reg_idx(cb.rs1),
                REG_ZERO as u8,
                branch_offset(cb),
            ),
            // C.ADDIW expands to `addiw rd, rd, imm[5:0]`
            #[cfg(feature = "64-bit")]
            Self::CAddiw(ci) => {
                expansion(Opcode::Addiw, ci.rs1_rd, ci.rs1_rd, 0, sext(ci.imm as i32, 6))
            }
        }
    }
}

/// Returns the offset of a C.J or C.JAL: `offset[11|4|9:8|10|6|7|3:1|5]`.
const fn jump_offset(cj: CJType) -> i32 {
    let offset = twiddle!(i32, cj.target, 10..11, 6..7, 7..9, 4..5, 5..6, 0..1, 9..10, 1..4) << 1;
    sext(offset, 12)
}

/// Returns the offset of a C.BEQZ or C.BNEZ: `offset[8|4:3]` and `offset[7:6|2:1|5]`.
const fn branch_offset(cb: CBType) -> i32 {
    sext(twiddle!(i32, cb.offset, 7..8, 3..5, 0..1, 5..7, 1..3) << 1, 9)
}

/// Returns the six bits `imm[5]` and `imm[4:0]` of a C.SRLI, C.SRAI or C.ANDI.
const fn cb_immediate(cb: CBType) -> i32 {
    twiddle!(i32, cb.offset, 7..8, 0..5)
}

/// Sub-functions of the [`C1`] `4` funct3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C1SubFunct {
    /// C.SRLI instruction.
    CSrli(CBType),
    /// C.SRAI instruction.
    CSrai(CBType),
    /// C.ANDI instruction.
    CAndi(CBType),
    /// C.SUB instruction.
    CSub(CSType),
    /// C.XOR instruction.
    CXor(CSType),
    /// C.OR instruction.
    COr(CSType),
    /// C.AND instruction.
    CAnd(CSType),
    /// C.SUBW instruction.
    #[cfg(feature = "64-bit")]
    CSubw(CSType),
    /// C.ADDW instruction.
    #[cfg(feature = "64-bit")]
    CAddw(CSType),
}

impl C1SubFunct {
    /// Decodes a [`C1SubFunct`] instruction from a 16-bit [`HalfWord`].
    pub const fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct6_low = bits!(u8, instruction, 10..12);
        let bit_12 = bits!(u8, instruction, 12..13);

        match funct6_low {
            // A 32-bit architecture has no shift amounts of 32 and more.
            0b00 | 0b01 if cfg!(not(feature = "64-bit")) && bit_12 != 0 => {
                Err(InstructionDecodeError::Reserved)
            }
            0b00 => Ok(Self::CSrli(CBType::decode(instruction))),
            0b01 => Ok(Self::CSrai(CBType::decode(instruction))),
            0b10 => Ok(Self::CAndi(CBType::decode(instruction))),
            _ => {
                let funct2 = bits!(u8, instruction, 5..7);
                let cs = CSType::decode(instruction);
                match (bit_12, funct2) {
                    (0, 0b00) => Ok(Self::CSub(cs)),
                    (0, 0b01) => Ok(Self::CXor(cs)),
                    (0, 0b10) => Ok(Self::COr(cs)),
                    (0, 0b11) => Ok(Self::CAnd(cs)),
                    #[cfg(feature = "64-bit")]
                    (1, 0b00) => Ok(Self::CSubw(cs)),
                    #[cfg(feature = "64-bit")]
                    (1, 0b01) => Ok(Self::CAddw(cs)),
                    _ => Err(invalid_function(funct2, bit_12)),
                }
            }
        }
    }

    /// Maps the compressed instruction to the [`Instruction`] it abbreviates.
    pub const fn expand(self) -> Instruction {
        // All of these have `rd'` as their destination and first source.
        let (opcode, rd, rs2, imm) = match self {
            // C.SRLI expands to `srli rd', rd', shamt[5:0]`
            Self::CSrli(cb) => (Opcode::Srli, cb.rs1, 0, cb_immediate(cb)),
            // C.SRAI expands to `srai rd', rd', shamt[5:0]`
            Self::CSrai(cb) => (Opcode::Srai, cb.rs1, 0, cb_immediate(cb)),
            // C.ANDI expands to `andi rd', rd', imm[5:0]`
            Self::CAndi(cb) => (Opcode::Andi, cb.rs1, 0, sext(cb_immediate(cb), 6)),
            // C.SUB expands to `sub rd', rd', rs2'`
            Self::CSub(cs) => (Opcode::Sub, cs.rs1, map_compressed_reg_idx(cs.rs2), 0),
            // C.XOR expands to `xor rd', rd', rs2'`
            Self::CXor(cs) => (Opcode::Xor, cs.rs1, map_compressed_reg_idx(cs.rs2), 0),
            // C.OR expands to `or rd', rd', rs2'`
            Self::COr(cs) => (Opcode::Or, cs.rs1, map_compressed_reg_idx(cs.rs2), 0),
            // C.AND expands to `and rd', rd', rs2'`
            Self::CAnd(cs) => (Opcode::And, cs.rs1, map_compressed_reg_idx(cs.rs2), 0),
            // C.SUBW expands to `subw rd', rd', rs2'`
            #[cfg(feature = "64-bit")]
            Self::CSubw(cs) => (Opcode::Subw, cs.rs1, map_compressed_reg_idx(cs.rs2), 0),
            // C.ADDW expands to `addw rd', rd', rs2'`
            #[cfg(feature = "64-bit")]
            Self::CAddw(cs) => (Opcode::Addw, cs.rs1, map_compressed_reg_idx(cs.rs2), 0),
        };
        let rd = map_compressed_reg_idx(rd);
        expansion(opcode, rd, rd, rs2, imm)
    }
}

/// A RISC-V C2 instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C2 {
    /// C.SLLI instruction.
    CSlli(CIType),
    /// C.LWSP instruction.
    CLwsp(CIType),
    /// C.SWSP instruction.
    CSwsp(CSSType),
    /// Sub-functions.
    SubFunct(C2SubFunct),
    /// C.LDSP instruction.
    #[cfg(feature = "64-bit")]
    CLdsp(CIType),
    /// C.SDSP instruction.
    #[cfg(feature = "64-bit")]
    CSdsp(CSSType),
}

impl C2 {
    /// Decodes a [`C2`] instruction from a 16-bit [`HalfWord`].
    pub const fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct3 = bits!(u8, instruction, 13..16);
        let ci = CIType::decode(instruction);

        match funct3 {
            // A 32-bit architecture has no shift amounts of 32 and more.
            0b000 if cfg!(not(feature = "64-bit")) && ci.imm >= 32 => {
                Err(InstructionDecodeError::Reserved)
            }
            0b000 => Ok(Self::CSlli(ci)),
            // The loads need a destination other than `x0`.
            0b010 | 0b011 if ci.rs1_rd == 0 => Err(InstructionDecodeError::Reserved),
            0b010 => Ok(Self::CLwsp(ci)),
            0b100 => Ok(Self::SubFunct(tri!(C2SubFunct::decode(instruction)))),
            0b110 => Ok(Self::CSwsp(CSSType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b011 => Ok(Self::CLdsp(ci)),
            #[cfg(feature = "64-bit")]
            0b111 => Ok(Self::CSdsp(CSSType::decode(instruction))),
            _ => Err(invalid_function(funct3, 0)),
        }
    }

    /// Maps the compressed instruction to the [`Instruction`] it abbreviates.
    pub const fn expand(self) -> Instruction {
        match self {
            // C.SLLI expands to `slli rd, rd, shamt[5:0]`
            Self::CSlli(ci) => expansion(Opcode::Slli, ci.rs1_rd, ci.rs1_rd, 0, ci.imm as i32),
            // C.LWSP expands to `lw rd, offset[7:2](x2)`
            Self::CLwsp(ci) => expansion(
                Opcode::Lw,
                ci.rs1_rd,
                REG_SP as u8,
                0,
                twiddle!(i32, ci.imm, 0..2, 2..6) << 2,
            ),
            // C.SWSP expands to `sw rs2, offset[7:2](x2)`
            Self::CSwsp(css) => expansion(
                Opcode::Sw,
                0,
                REG_SP as u8,
                css.rs2,
                twiddle!(i32, css.imm, 0..2, 2..6) << 2,
            ),
            Self::SubFunct(sub_funct) => sub_funct.expand(),
            // C.LDSP expands to `ld rd, offset[8:3](x2)`
            #[cfg(feature = "64-bit")]
            Self::CLdsp(ci) => expansion(
                Opcode::Ld,
                ci.rs1_rd,
                REG_SP as u8,
                0,
                twiddle!(i32, ci.imm, 0..3, 3..6) << 3,
            ),
            // C.SDSP expands to `sd rs2, offset[8:3](x2)`
            #[cfg(feature = "64-bit")]
            Self::CSdsp(css) => expansion(
                Opcode::Sd,
                0,
                REG_SP as u8,
                css.rs2,
                twiddle!(i32, css.imm, 0..3, 3..6) << 3,
            ),
        }
    }
}

/// Sub-functions of the [`C2`] `4` funct3.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum C2SubFunct {
    /// C.JR instruction.
    CJr(CRType),
    /// C.MV instruction.
    CMv(CRType),
    /// C.EBREAK instruction.
    CEBreak,
    /// C.JALR instruction.
    CJalr(CRType),
    /// C.ADD instruction.
    CAdd(CRType),
}

impl C2SubFunct {
    /// Decodes a [`C2SubFunct`] instruction from a 16-bit [`HalfWord`].
    pub const fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let cr = CRType::decode(instruction);

        // The lowest bit of funct4 and whether the register fields are zero select the function.
        match (cr.funct4 & 1, cr.rs2, cr.rs1_rd) {
            // C.JR needs a register other than `x0` to jump to.
            (0, 0, 0) => Err(InstructionDecodeError::Reserved),
            (0, 0, _) => Ok(Self::CJr(cr)),
            (0, _, _) => Ok(Self::CMv(cr)),
            (_, 0, 0) => Ok(Self::CEBreak),
            (_, 0, _) => Ok(Self::CJalr(cr)),
            (_, _, _) => Ok(Self::CAdd(cr)),
        }
    }

    /// Maps the compressed instruction to the [`Instruction`] it abbreviates.
    pub const fn expand(self) -> Instruction {
        match self {
            // C.JR expands to `jalr x0, 0(rs1)`
            Self::CJr(cr) => expansion(Opcode::Jalr, REG_ZERO as u8, cr.rs1_rd, 0, 0),
            // C.MV expands to `add rd, x0, rs2`
            Self::CMv(cr) => expansion(Opcode::Add, cr.rs1_rd, REG_ZERO as u8, cr.rs2, 0),
            // C.EBREAK expands to `ebreak`
            Self::CEBreak => expansion(Opcode::Ebreak, 0, 0, 0, 0),
            // C.JALR expands to `jalr x1, 0(rs1)`
            Self::CJalr(cr) => expansion(Opcode::Jalr, REG_RA as u8, cr.rs1_rd, 0, 0),
            // C.ADD expands to `add rd, rd, rs2`
            Self::CAdd(cr) => expansion(Opcode::Add, cr.rs1_rd, cr.rs1_rd, cr.rs2, 0),
        }
    }
}

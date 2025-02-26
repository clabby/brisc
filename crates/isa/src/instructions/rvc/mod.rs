//! Compressed instructions, from the RISC-V `c` extension.

use super::{BType, JType, UType};
use crate::{
    bits, sign_extend, twiddle, BranchFunction, EnvironmentFunction, HalfWord, IType,
    ImmediateArithmeticFunction, Instruction, InstructionDecodeError, LoadFunction, RType,
    RegisterArithmeticFunction, SType, StoreFunction, Word, XWord, REG_RA, REG_SP, REG_ZERO,
};
use cfg_if::cfg_if;

#[cfg(feature = "64-bit")]
use crate::{ImmediateArithmeticWordFunction, RegisterArithmeticWordFunction};

mod types;
pub use types::*;

/// C_REGISTER_OFFSET is the offset of the register mapping from the `C` instructions to regular
/// 32 bit instructions. In the `C` extension, register fields are only allotted 3 bits,
/// allowing for 8 possible register designations.
pub const C_REG_OFFSET: u8 = 8;

/// Returns `true` if the given instruction is compressed. In RISC-V, compressed instructions
/// are identified by the two least significant bits being set.
pub const fn is_compressed(instr: Word) -> bool {
    instr & 0b11 != 0b11
}

/// Maps a compressed register index to a regular register index, by adding [C_REG_OFFSET].
/// ┌─────┬─────┬─────┬─────┬─────┬─────┬─────┬─────┐
/// │ 000 │ 001 │ 010 │ 011 │ 100 │ 101 │ 110 │ 111 │
/// ├─────┼─────┼─────┼─────┼─────┼─────┼─────┼─────┤
/// │ x8  │ x9  │ x10 │ x11 │ x12 │ x13 │ x14 │ x15 │
/// └─────┴─────┴─────┴─────┴─────┴─────┴─────┴─────┘
pub const fn map_compressed_reg_idx(reg: u8) -> u8 {
    reg + C_REG_OFFSET
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
    /// Decodes a [CompressedInstruction] from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let opcode = instruction & 0b11;
        match opcode {
            0b00 => Ok(Self::C0(C0::decode(instruction)?)),
            0b01 => Ok(Self::C1(C1::decode(instruction)?)),
            0b10 => Ok(Self::C2(C2::decode(instruction)?)),
            _ => Err(InstructionDecodeError::InvalidOpcode(opcode as u8)),
        }
    }

    /// Maps the [CompressedInstruction] to a regular RISC-V [Instruction].
    pub fn expand(self) -> Instruction {
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
    /// C.SW instruction.
    #[cfg(feature = "64-bit")]
    CLd(CLType),
    /// C.SD instruction.
    #[cfg(feature = "64-bit")]
    CSd(CSType),
}

impl C0 {
    /// Decodes a [C0] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct3 = bits!(u8, instruction, 13..16);
        match funct3 {
            0b000 => Ok(Self::CAddi4spn(CIWType::decode(instruction))),
            0b010 => Ok(Self::CLw(CLType::decode(instruction))),
            0b110 => Ok(Self::CSw(CSType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b011 => Ok(Self::CLd(CLType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b111 => Ok(Self::CSd(CSType::decode(instruction))),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: funct3, q_b: 0 }),
        }
    }

    /// Maps the compressed instruction to a regular RISC-V [Instruction].
    pub fn expand(self) -> Instruction {
        match self {
            Self::CAddi4spn(ciw) => {
                // C.ADDI4SPN expands to `addi rd', x2, nzuimm[9:2]`
                let nzuimm = twiddle!(XWord, ciw.imm, 2..4, 4..8, 0..1, 1..2) << 2;
                let i_type = IType {
                    rd: map_compressed_reg_idx(ciw.rd),
                    funct3: 0,
                    rs1: REG_SP as u8,
                    imm: nzuimm,
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Addi)
            }
            Self::CLw(cl) => {
                // C.LW expands to `lw rd', offset[6:2](rs1')`
                let i_type = IType {
                    rd: map_compressed_reg_idx(cl.rd),
                    funct3: 0b010,
                    rs1: map_compressed_reg_idx(cl.rs1),
                    imm: twiddle!(XWord, cl.imm, 0..1, 2..5, 1..2) << 2,
                };
                Instruction::MemoryLoad(i_type, LoadFunction::Lw)
            }
            Self::CSw(cs) => {
                // C.SW expands to `sw rs2', offset[6:2](rs1')`
                let s_type = SType {
                    funct3: 0b010,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    imm: twiddle!(XWord, cs.imm, 0..1, 2..5, 1..2) << 2,
                };
                Instruction::MemoryStore(s_type, StoreFunction::Sw)
            }
            #[cfg(feature = "64-bit")]
            Self::CLd(cl) => {
                // C.LD expands to `ld rd', offset[7:3](rs1')`
                let i_type = IType {
                    rd: map_compressed_reg_idx(cl.rd),
                    funct3: cl.funct3,
                    rs1: map_compressed_reg_idx(cl.rs1),
                    imm: twiddle!(XWord, cl.imm, 0..2, 2..5) << 3,
                };
                Instruction::MemoryLoad(i_type, LoadFunction::Ld)
            }
            #[cfg(feature = "64-bit")]
            Self::CSd(cs) => {
                // C.SD expands to `sd rs2', offset[7:3](rs1')`
                let s_type = SType {
                    funct3: cs.funct3,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    imm: twiddle!(XWord, cs.imm, 0..2, 2..5) << 3,
                };
                Instruction::MemoryStore(s_type, StoreFunction::Sd)
            }
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
    /// Decodes a [C1] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct3 = bits!(u8, instruction, 13..16);
        let rs1_rd = bits!(u8, instruction, 7..12);

        match funct3 {
            0b000 => Ok(Self::CAddi(CIType::decode(instruction))),
            0b001 => {
                cfg_if! {
                    // In 64-bit mode, C.ADDIW is used instead of C.JAL.
                    if #[cfg(feature = "64-bit")] {
                        if rs1_rd == 0 {
                            return Err(InstructionDecodeError::InvalidFunction { q_a: funct3, q_b: 0 });
                        } else {
                            Ok(Self::CAddiw(CIType::decode(instruction)))
                        }
                    } else {
                        Ok(Self::CJal(CJType::decode(instruction)))
                    }
                }
            }
            0b010 => Ok(Self::CLi(CIType::decode(instruction))),
            0b011 => {
                if rs1_rd == 2 {
                    Ok(Self::CAddi16sp(CIType::decode(instruction)))
                } else if rs1_rd != 0 {
                    Ok(Self::CLui(CIType::decode(instruction)))
                } else {
                    Err(InstructionDecodeError::InvalidFunction { q_a: funct3, q_b: 0 })
                }
            }
            0b100 => Ok(Self::SubFunct(C1SubFunct::decode(instruction)?)),
            0b101 => Ok(Self::CJ(CJType::decode(instruction))),
            0b110 => Ok(Self::CBeqz(CBType::decode(instruction))),
            0b111 => Ok(Self::CBnez(CBType::decode(instruction))),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: funct3, q_b: 0 }),
        }
    }

    /// Maps the compressed instruction to a regular RISC-V [Instruction].
    pub fn expand(self) -> Instruction {
        match self {
            Self::CAddi(ci) => {
                // C.ADDI expands to `addi rd, rd, imm[5:0]`
                let i_type = IType {
                    rd: ci.rs1_rd,
                    funct3: 0,
                    rs1: ci.rs1_rd,
                    imm: sign_extend(ci.imm as XWord, 5),
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Addi)
            }
            Self::CJal(cj) => {
                // C.JAL expands to `jal x1, imm[11:1]`
                let target = twiddle!(
                    XWord,
                    cj.target,
                    10..11,
                    6..7,
                    7..9,
                    4..5,
                    5..6,
                    0..1,
                    9..10,
                    1..4
                ) << 1;
                let j_type = JType { rd: REG_RA as u8, imm: sign_extend(target, 11) };
                Instruction::Jal(j_type)
            }
            Self::CLi(ci) => {
                // C.LI expands to `addi rd, x0, imm[5:0]`
                let i_type = IType {
                    rd: ci.rs1_rd,
                    funct3: 0,
                    rs1: REG_ZERO as u8,
                    imm: sign_extend(ci.imm as XWord, 5),
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Addi)
            }
            Self::CAddi16sp(ci) => {
                // C.ADDI16SP expands to `addi x2, x2, nzimm[9:4]`
                let nzimm = twiddle!(XWord, ci.imm, 5..6, 1..3, 3..4, 0..1, 4..5) << 4;
                let i_type = IType {
                    rd: REG_SP as u8,
                    funct3: 0,
                    rs1: REG_SP as u8,
                    imm: sign_extend(nzimm, 9),
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Addi)
            }
            Self::CLui(ci) => {
                // C.LUI expands to `lui rd, imm[17:12]`
                let u_type = UType { rd: ci.rs1_rd, imm: sign_extend((ci.imm as XWord) << 12, 17) };
                Instruction::Lui(u_type)
            }
            Self::SubFunct(sub_funct) => sub_funct.map(),
            Self::CJ(cj) => {
                // C.J expands to `jal x0, offset[11:1]`
                let target = twiddle!(
                    XWord,
                    cj.target,
                    10..11,
                    6..7,
                    7..9,
                    4..5,
                    5..6,
                    0..1,
                    9..10,
                    1..4
                ) << 1;
                let j_type = JType { rd: REG_ZERO as u8, imm: sign_extend(target, 11) };
                Instruction::Jal(j_type)
            }
            Self::CBeqz(cb) => {
                // C.BEQZ expands to `beq rs1', x0, offset[8:1]`
                let target = twiddle!(XWord, cb.offset, 7..8, 3..5, 0..1, 5..7, 1..3) << 1;
                let b_type = BType {
                    funct3: 0,
                    rs1: map_compressed_reg_idx(cb.rs1),
                    rs2: REG_ZERO as u8,
                    imm: sign_extend(target, 8),
                };
                Instruction::Branch(b_type, BranchFunction::Beq)
            }
            Self::CBnez(cb) => {
                // C.BNEZ expands to `bne rs1', x0, offset[8:1]`
                let target = twiddle!(XWord, cb.offset, 7..8, 3..5, 0..1, 5..7, 1..3) << 1;
                let b_type = BType {
                    funct3: 1,
                    rs1: map_compressed_reg_idx(cb.rs1),
                    rs2: REG_ZERO as u8,
                    imm: sign_extend(target, 8),
                };
                Instruction::Branch(b_type, BranchFunction::Bne)
            }
            #[cfg(feature = "64-bit")]
            Self::CAddiw(ci) => {
                // C.ADDIW expands to `addiw rd', rd', imm[5:0]`
                let i_type = IType {
                    rd: ci.rs1_rd,
                    funct3: 0,
                    rs1: ci.rs1_rd,
                    imm: sign_extend(ci.imm as XWord, 5),
                };
                Instruction::ImmediateArithmeticWord(i_type, ImmediateArithmeticWordFunction::Addiw)
            }
        }
    }
}

/// Sub-functions of the [C1] `4` funct3.
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
    /// Decodes a [C1SubFunct] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct6_low = bits!(u8, instruction, 10..12);

        match funct6_low {
            0b00 => Ok(Self::CSrli(CBType::decode(instruction))),
            0b01 => Ok(Self::CSrai(CBType::decode(instruction))),
            0b10 => Ok(Self::CAndi(CBType::decode(instruction))),
            0b11 => {
                let funct2 = bits!(u8, instruction, 5..7);
                let arch_sel = bits!(u8, instruction, 12..13);
                match funct2 {
                    0b00 if arch_sel == 0 => Ok(Self::CSub(CSType::decode(instruction))),
                    0b01 if arch_sel == 0 => Ok(Self::CXor(CSType::decode(instruction))),
                    0b10 => Ok(Self::COr(CSType::decode(instruction))),
                    0b11 => Ok(Self::CAnd(CSType::decode(instruction))),
                    #[cfg(feature = "64-bit")]
                    0b00 if arch_sel == 1 => Ok(Self::CSubw(CSType::decode(instruction))),
                    #[cfg(feature = "64-bit")]
                    0b01 if arch_sel == 1 => Ok(Self::CAddw(CSType::decode(instruction))),
                    _ => {
                        Err(InstructionDecodeError::InvalidFunction { q_a: funct2, q_b: arch_sel })
                    }
                }
            }
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: funct6_low, q_b: 0 }),
        }
    }

    /// Maps the compressed instruction to a regular RISC-V [Instruction].
    pub fn map(self) -> Instruction {
        match self {
            Self::CSrli(cb) => {
                // C.SRLI expands to `srli rd', rd', shamt[5:0]`
                let i_type = IType {
                    rd: map_compressed_reg_idx(cb.rs1),
                    funct3: 0b101,
                    rs1: map_compressed_reg_idx(cb.rs1),
                    imm: twiddle!(XWord, cb.offset, 7..8, 0..5),
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Srli)
            }
            Self::CSrai(cb) => {
                // C.SRAI expands to `srai rd', rd', shamt[5:0]`
                let i_type = IType {
                    rd: map_compressed_reg_idx(cb.rs1),
                    funct3: 0b101,
                    rs1: map_compressed_reg_idx(cb.rs1),
                    imm: twiddle!(XWord, cb.offset, 7..8, 0..5) | (0x20 << 5),
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Srai)
            }
            Self::CAndi(cb) => {
                // C.ANDI expands to `andi rd', rd', imm[5:0]`
                let i_type = IType {
                    rd: map_compressed_reg_idx(cb.rs1),
                    funct3: 0b111,
                    rs1: map_compressed_reg_idx(cb.rs1),
                    imm: sign_extend(twiddle!(XWord, cb.offset, 7..8, 0..5), 5),
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Andi)
            }
            Self::CSub(cs) => {
                // C.SUB expands to `sub rd', rd', rs2'`
                let r_type = RType {
                    rd: map_compressed_reg_idx(cs.rs1),
                    funct3: 0b000,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    funct7: 0x20,
                };
                Instruction::RegisterArithmetic(r_type, RegisterArithmeticFunction::Sub)
            }
            Self::CXor(cs) => {
                // C.XOR expands to `xor rd', rd', rs2'`
                let r_type = RType {
                    rd: map_compressed_reg_idx(cs.rs1),
                    funct3: 0b100,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    funct7: 0,
                };
                Instruction::RegisterArithmetic(r_type, RegisterArithmeticFunction::Xor)
            }
            Self::COr(cs) => {
                // C.OR expands to `or rd', rd', rs2'`
                let r_type = RType {
                    rd: map_compressed_reg_idx(cs.rs1),
                    funct3: 0b110,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    funct7: 0,
                };
                Instruction::RegisterArithmetic(r_type, RegisterArithmeticFunction::Or)
            }
            Self::CAnd(cs) => {
                // C.AND expands to `and rd', rd', rs2'`
                let r_type = RType {
                    rd: map_compressed_reg_idx(cs.rs1),
                    funct3: 0b111,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    funct7: 0,
                };
                Instruction::RegisterArithmetic(r_type, RegisterArithmeticFunction::And)
            }
            #[cfg(feature = "64-bit")]
            Self::CSubw(cs) => {
                // C.SUBW expands to `subw rd', rd', rs2'`
                let r_type = RType {
                    rd: map_compressed_reg_idx(cs.rs1),
                    funct3: 0b000,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    funct7: 0x20,
                };
                Instruction::RegisterArithmeticWord(r_type, RegisterArithmeticWordFunction::Subw)
            }
            #[cfg(feature = "64-bit")]
            Self::CAddw(cs) => {
                // C.ADDW expands to `addw rd', rd', rs2'`
                let r_type = RType {
                    rd: map_compressed_reg_idx(cs.rs1),
                    funct3: 0b000,
                    rs1: map_compressed_reg_idx(cs.rs1),
                    rs2: map_compressed_reg_idx(cs.rs2),
                    funct7: 0,
                };
                Instruction::RegisterArithmeticWord(r_type, RegisterArithmeticWordFunction::Addw)
            }
        }
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
    /// Decodes a [C2] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let funct3 = bits!(u8, instruction, 13..16);
        let rd = bits!(u8, instruction, 7..12);

        match funct3 {
            0b000 if rd != 0 => Ok(Self::CSlli(CIType::decode(instruction))),
            0b010 if rd != 0 => Ok(Self::CLwsp(CIType::decode(instruction))),
            0b100 => Ok(Self::SubFunct(C2SubFunct::decode(instruction)?)),
            0b110 => Ok(Self::CSwsp(CSSType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b011 if rd != 0 => Ok(Self::CLdsp(CIType::decode(instruction))),
            #[cfg(feature = "64-bit")]
            0b111 => Ok(Self::CSdsp(CSSType::decode(instruction))),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: funct3, q_b: 0 }),
        }
    }

    /// Maps the compressed instruction to a regular RISC-V [Instruction].
    pub fn expand(self) -> Instruction {
        match self {
            Self::CSlli(ci) => {
                // C.SLLI expands to `slli rd, rd, shamt[5:0]`
                let i_type = IType {
                    rd: ci.rs1_rd,
                    funct3: 1,
                    rs1: ci.rs1_rd,
                    imm: (ci.imm & 0x3F) as XWord,
                };
                Instruction::ImmediateArithmetic(i_type, ImmediateArithmeticFunction::Slli)
            }
            Self::CLwsp(ci) => {
                // C.LWSP expands to `lw rd, offset[7:2](x2)`
                let i_type = IType {
                    rd: ci.rs1_rd,
                    funct3: 2,
                    rs1: REG_SP as u8,
                    imm: twiddle!(XWord, ci.imm as XWord, 0..2, 2..6) << 2,
                };
                Instruction::MemoryLoad(i_type, LoadFunction::Lw)
            }
            Self::CSwsp(css) => {
                // C.SWSP expands to `sw rs2, offset[7:2](x2)`
                let s_type = SType {
                    funct3: 2,
                    rs1: REG_SP as u8,
                    rs2: css.rs2,
                    imm: twiddle!(XWord, css.imm as XWord, 0..2, 2..6) << 2,
                };
                Instruction::MemoryStore(s_type, StoreFunction::Sw)
            }
            Self::SubFunct(sf) => sf.map(),
            #[cfg(feature = "64-bit")]
            Self::CLdsp(ci) => {
                // C.LDSP expands to `ld rd, offset[8:3](x2)`
                let i_type = IType {
                    rd: ci.rs1_rd,
                    funct3: 3,
                    rs1: REG_SP as u8,
                    imm: twiddle!(XWord, ci.imm as XWord, 0..3, 3..6) << 3,
                };
                Instruction::MemoryLoad(i_type, LoadFunction::Ld)
            }
            #[cfg(feature = "64-bit")]
            Self::CSdsp(css) => {
                // C.SDSP expands to `sd rs2, offset[8:3](x2)`
                let s_type = SType {
                    funct3: 3,
                    rs1: REG_SP as u8,
                    rs2: css.rs2,
                    imm: twiddle!(XWord, css.imm as XWord, 0..3, 3..6) << 3,
                };
                Instruction::MemoryStore(s_type, StoreFunction::Sd)
            }
        }
    }
}

/// Sub-functions of the [C2] `4` funct3.
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
    /// Decodes a [C2SubFunct] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Result<Self, InstructionDecodeError> {
        let sel = bits!(u8, instruction, 12..13);
        let cr = CRType::decode(instruction);

        match sel {
            0 => match cr.rs2 {
                0 => Ok(Self::CJr(cr)),
                _ => Ok(Self::CMv(cr)),
            },
            1 => match (cr.rs2, cr.rs1_rd) {
                (0, 0) => Ok(Self::CEBreak),
                (0, rs1_rd) if rs1_rd != 0 => Ok(Self::CJalr(cr)),
                (rs2, rs1_rd) if rs2 != 0 && rs1_rd != 0 => Ok(Self::CAdd(cr)),
                _ => Err(InstructionDecodeError::InvalidFunction { q_a: sel, q_b: 0 }),
            },
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: sel, q_b: 0 }),
        }
    }

    /// Maps the compressed instruction to a regular RISC-V [Instruction].
    pub fn map(self) -> Instruction {
        match self {
            Self::CJr(cr) => {
                // C.JR expands to `jalr x0, rs1, 0`
                let i_type = IType { rd: REG_ZERO as u8, funct3: 0, rs1: cr.rs1_rd, imm: 0 };
                Instruction::Jalr(i_type)
            }
            Self::CMv(cr) => {
                // C.MV expands to `add rd, x0, rs2`
                let r_type =
                    RType { rd: cr.rs1_rd, funct3: 0, rs1: REG_ZERO as u8, rs2: cr.rs2, funct7: 0 };
                Instruction::RegisterArithmetic(r_type, RegisterArithmeticFunction::Add)
            }
            Self::CJalr(cr) => {
                // C.JALR expands to `jalr x1, rs1, 0`
                let i_type = IType { rd: REG_RA as u8, funct3: 0, rs1: cr.rs1_rd, imm: 0 };
                Instruction::Jalr(i_type)
            }
            Self::CAdd(cr) => {
                // C.ADD expands to `add rd, rd, rs2`
                let r_type =
                    RType { rd: cr.rs1_rd, funct3: 0, rs1: cr.rs1_rd, rs2: cr.rs2, funct7: 0 };
                Instruction::RegisterArithmetic(r_type, RegisterArithmeticFunction::Add)
            }
            Self::CEBreak => {
                // C.EBREAK expands to `ebreak`
                Instruction::Environment(IType::default(), EnvironmentFunction::Ebreak)
            }
        }
    }
}

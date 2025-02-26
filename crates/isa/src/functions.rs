//! Contains function definitions for the ISA.

use crate::{bits, BType, IType, InstructionDecodeError, RType, SType};

/// Functions for Integer Register-Register Instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegisterArithmeticFunction {
    /// The `ADD` function.
    Add,
    /// The `SUB` function.
    Sub,
    /// The `XOR` function.
    Xor,
    /// The `OR` function.
    Or,
    /// The `AND` function.
    And,
    /// The `SLL` function.
    Sll,
    /// The `SRL` function.
    Srl,
    /// The `SRA` function.
    Sra,
    /// The `SLT` function.
    Slt,
    /// The `SLTU` function.
    Sltu,
    /// The `MUL` function.
    #[cfg(feature = "m")]
    Mul,
    /// The `MULH` function.
    #[cfg(feature = "m")]
    Mulh,
    /// The `MULHSU` function.
    #[cfg(feature = "m")]
    Mulhsu,
    /// The `MULHU` function.
    #[cfg(feature = "m")]
    Mulhu,
    /// The `DIV` function.
    #[cfg(feature = "m")]
    Div,
    /// The `DIVU` function.
    #[cfg(feature = "m")]
    Divu,
    /// The `REM` function.
    #[cfg(feature = "m")]
    Rem,
    /// The `REMU` function.
    #[cfg(feature = "m")]
    Remu,
}

impl TryFrom<&RType> for RegisterArithmeticFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &RType) -> Result<Self, Self::Error> {
        match (value.funct3, value.funct7) {
            (0x00, 0x00) => Ok(Self::Add),
            (0x00, 0x20) => Ok(Self::Sub),
            (0x04, 0x00) => Ok(Self::Xor),
            (0x06, 0x00) => Ok(Self::Or),
            (0x07, 0x00) => Ok(Self::And),
            (0x01, 0x00) => Ok(Self::Sll),
            (0x05, 0x00) => Ok(Self::Srl),
            (0x05, 0x20) => Ok(Self::Sra),
            (0x02, 0x00) => Ok(Self::Slt),
            (0x03, 0x00) => Ok(Self::Sltu),
            #[cfg(feature = "m")]
            (0x00, 0x01) => Ok(Self::Mul),
            #[cfg(feature = "m")]
            (0x01, 0x01) => Ok(Self::Mulh),
            #[cfg(feature = "m")]
            (0x02, 0x01) => Ok(Self::Mulhsu),
            #[cfg(feature = "m")]
            (0x03, 0x01) => Ok(Self::Mulhu),
            #[cfg(feature = "m")]
            (0x04, 0x01) => Ok(Self::Div),
            #[cfg(feature = "m")]
            (0x05, 0x01) => Ok(Self::Divu),
            #[cfg(feature = "m")]
            (0x06, 0x01) => Ok(Self::Rem),
            #[cfg(feature = "m")]
            (0x07, 0x01) => Ok(Self::Remu),
            _ => Err(InstructionDecodeError::InvalidFunction {
                q_a: value.funct3,
                q_b: value.funct7,
            }),
        }
    }
}

/// Functions for Integer Register-Register Word Instructions.
#[cfg(feature = "64-bit")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RegisterArithmeticWordFunction {
    /// The `ADDW` function.
    Addw,
    /// The `SUBW` function.
    Subw,
    /// The `SLLW` function.
    Sllw,
    /// The `SRLW` function.
    Srlw,
    /// The `SRAW` function.
    Sraw,
    /// The `MULW` function.
    #[cfg(feature = "m")]
    Mulw,
    /// The `DIVW` function.
    #[cfg(feature = "m")]
    Divw,
    /// The `DIVUW` function.
    #[cfg(feature = "m")]
    Divuw,
    /// The `REMW` function.
    #[cfg(feature = "m")]
    Remw,
    /// The `REMUW` function.
    #[cfg(feature = "m")]
    Remuw,
}

#[cfg(feature = "64-bit")]
impl TryFrom<&RType> for RegisterArithmeticWordFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &RType) -> Result<Self, Self::Error> {
        match (value.funct3, value.funct7) {
            (0x00, 0x00) => Ok(Self::Addw),
            (0x00, 0x20) => Ok(Self::Subw),
            (0x01, 0x00) => Ok(Self::Sllw),
            (0x05, 0x00) => Ok(Self::Srlw),
            (0x05, 0x20) => Ok(Self::Sraw),
            #[cfg(feature = "m")]
            (0x00, 0x01) => Ok(Self::Mulw),
            #[cfg(feature = "m")]
            (0x04, 0x01) => Ok(Self::Divw),
            #[cfg(feature = "m")]
            (0x05, 0x01) => Ok(Self::Divuw),
            #[cfg(feature = "m")]
            (0x06, 0x01) => Ok(Self::Remw),
            #[cfg(feature = "m")]
            (0x07, 0x01) => Ok(Self::Remuw),
            _ => Err(InstructionDecodeError::InvalidFunction {
                q_a: value.funct3,
                q_b: value.funct7,
            }),
        }
    }
}

/// Functions for Integer Register-Immediate Instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImmediateArithmeticFunction {
    /// The `ADDI` function.
    Addi,
    /// The `XORI` function.
    Xori,
    /// The `ORI` function.
    Ori,
    /// The `ANDI` function.
    Andi,
    /// The `SLLI` function.
    Slli,
    /// The `SRLI` function.
    Srli,
    /// The `SRAI` function.
    Srai,
    /// The `SLTI` function.
    Slti,
    /// The `SLTIU` function.
    Sltiu,
}

impl TryFrom<&IType> for ImmediateArithmeticFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &IType) -> Result<Self, Self::Error> {
        match value.funct3 {
            0x00 => Ok(Self::Addi),
            0x01 => Ok(Self::Slli),
            0x02 => Ok(Self::Slti),
            0x03 => Ok(Self::Sltiu),
            0x04 => Ok(Self::Xori),
            #[cfg(not(feature = "64-bit"))]
            0x05 if bits!(u8, value.imm, 5..12) == 0 => Ok(Self::Srli),
            #[cfg(not(feature = "64-bit"))]
            0x05 if bits!(u8, value.imm, 5..12) == 0x20 => Ok(Self::Srai),
            #[cfg(feature = "64-bit")]
            0x05 if bits!(u8, value.imm, 6..12) == 0 => Ok(Self::Srli),
            #[cfg(feature = "64-bit")]
            0x05 if bits!(u8, value.imm, 6..12) == 0x10 => Ok(Self::Srai),
            0x06 => Ok(Self::Ori),
            0x07 => Ok(Self::Andi),
            _ => Err(InstructionDecodeError::InvalidFunction {
                q_a: value.funct3,
                q_b: bits!(u8, value.imm, 5..12),
            }),
        }
    }
}

/// Functions for Integer Register-Immediate Word Instructions.
#[cfg(feature = "64-bit")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImmediateArithmeticWordFunction {
    /// The `ADDIW` function.
    Addiw,
    /// The `SLLIW` function.
    Slliw,
    /// The `SRLIW` function.
    Srliw,
    /// The `SRAIW` function.
    Sraiw,
}

#[cfg(feature = "64-bit")]
impl TryFrom<&IType> for ImmediateArithmeticWordFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &IType) -> Result<Self, Self::Error> {
        match value.funct3 {
            0x00 => Ok(Self::Addiw),
            0x01 if bits!(u8, value.imm, 5..12) == 0 => Ok(Self::Slliw),
            0x05 if bits!(u8, value.imm, 5..12) == 0 => Ok(Self::Srliw),
            0x05 if bits!(u8, value.imm, 5..12) == 0x20 => Ok(Self::Sraiw),
            _ => Err(InstructionDecodeError::InvalidFunction {
                q_a: value.funct3,
                q_b: bits!(u8, value.imm, 5..12),
            }),
        }
    }
}

/// Functions for Load Instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LoadFunction {
    /// The `LB` function.
    Lb,
    /// The `LH` function.
    Lh,
    /// The `LW` function.
    Lw,
    /// The `LBU` function.
    Lbu,
    /// The `LHU` function.
    Lhu,
    /// The `LWU` function.
    #[cfg(feature = "64-bit")]
    Lwu,
    /// The `LD` function.
    #[cfg(feature = "64-bit")]
    Ld,
}

impl TryFrom<&IType> for LoadFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &IType) -> Result<Self, Self::Error> {
        match value.funct3 {
            0x00 => Ok(Self::Lb),
            0x01 => Ok(Self::Lh),
            0x02 => Ok(Self::Lw),
            0x04 => Ok(Self::Lbu),
            0x05 => Ok(Self::Lhu),
            #[cfg(feature = "64-bit")]
            0x06 => Ok(Self::Lwu),
            #[cfg(feature = "64-bit")]
            0x03 => Ok(Self::Ld),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: value.funct3, q_b: 0 }),
        }
    }
}

/// Functions for Store Instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreFunction {
    /// The `SB` function.
    Sb,
    /// The `SH` function.
    Sh,
    /// The `SW` function.
    Sw,
    /// The `SD` function.
    #[cfg(feature = "64-bit")]
    Sd,
}

impl TryFrom<&SType> for StoreFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &SType) -> Result<Self, Self::Error> {
        match value.funct3 {
            0x00 => Ok(Self::Sb),
            0x01 => Ok(Self::Sh),
            0x02 => Ok(Self::Sw),
            #[cfg(feature = "64-bit")]
            0x03 => Ok(Self::Sd),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: value.funct3, q_b: 0 }),
        }
    }
}

/// Functions for Branch Instructions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BranchFunction {
    /// The `BEQ` function.
    Beq,
    /// The `BNE` function.
    Bne,
    /// The `BLT` function.
    Blt,
    /// The `BGE` function.
    Bge,
    /// The `BLTU` function.
    Bltu,
    /// The `BGEU` function.
    Bgeu,
}

impl TryFrom<&BType> for BranchFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &BType) -> Result<Self, Self::Error> {
        match value.funct3 {
            0x00 => Ok(Self::Beq),
            0x01 => Ok(Self::Bne),
            0x04 => Ok(Self::Blt),
            0x05 => Ok(Self::Bge),
            0x06 => Ok(Self::Bltu),
            0x07 => Ok(Self::Bgeu),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: value.funct3, q_b: 0 }),
        }
    }
}

/// Functions for Environment Instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvironmentFunction {
    /// The `ECALL` function.
    Ecall,
    /// The `EBREAK` function.
    Ebreak,
}

impl TryFrom<&IType> for EnvironmentFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &IType) -> Result<Self, Self::Error> {
        match value.funct3 {
            0x00 if value.imm == 0 => Ok(Self::Ecall),
            _ => Ok(Self::Ebreak),
            // _ => Err(InstructionDecodeError::InvalidFunction { q_a: value.funct3, q_b: 0 }),
        }
    }
}

/// Functions for the "A" extension.
#[cfg(feature = "a")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AmoFunction {
    /// The `LR` function.
    Lr,
    /// The `SC` function.
    Sc,
    /// The `AMOSWAP` function.
    Amoswap,
    /// The `AMOADD` function.
    Amoadd,
    /// The `AMOXOR` function.
    Amoxor,
    /// The `AMOAND` function.
    Amoand,
    /// The `AMOOR` function.
    Amoor,
    /// The `AMOMIN` function.
    Amomin,
    /// The `AMOMAX` function.
    Amomax,
    /// The `AMOMINU` function.
    Amominu,
    /// The `AMOMAXU` function.
    Amomaxu,
}

#[cfg(feature = "a")]
impl TryFrom<&RType> for AmoFunction {
    type Error = InstructionDecodeError;

    fn try_from(value: &RType) -> Result<Self, Self::Error> {
        let afunct5 = bits!(u8, value.funct7, 2..7);
        match afunct5 {
            0b00010 => Ok(Self::Lr),
            0b00011 => Ok(Self::Sc),
            0b00001 => Ok(Self::Amoswap),
            0b00000 => Ok(Self::Amoadd),
            0b00100 => Ok(Self::Amoxor),
            0b01100 => Ok(Self::Amoand),
            0b01000 => Ok(Self::Amoor),
            0b10000 => Ok(Self::Amomin),
            0b10100 => Ok(Self::Amomax),
            0b11000 => Ok(Self::Amominu),
            0b11100 => Ok(Self::Amomaxu),
            _ => Err(InstructionDecodeError::InvalidFunction { q_a: afunct5, q_b: 0 }),
        }
    }
}

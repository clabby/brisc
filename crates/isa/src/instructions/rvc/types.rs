//! RISC-V `c` extension instruction types.

use crate::{bits, twiddle, HalfWord};

/// A RISC-V CR-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CRType {
    /// The first source register or destination register.
    pub rs1_rd: u8,
    /// The second source register.
    pub rs2: u8,
    /// The funct4 field.
    pub funct4: u8,
}

impl CRType {
    /// Decodes a [CRType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rs1_rd: bits!(u8, instruction, 7..12),
            rs2: bits!(u8, instruction, 2..7),
            funct4: bits!(u8, instruction, 12..16),
        }
    }
}

/// A RISC-V CI-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CIType {
    /// The source register or destination register.
    pub rs1_rd: u8,
    /// The funct3 field.
    pub funct3: u8,
    /// The immediate.
    pub imm: HalfWord,
}

impl CIType {
    /// Decodes a [CIType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rs1_rd: bits!(u8, instruction, 7..12),
            funct3: bits!(u8, instruction, 13..16),
            imm: twiddle!(HalfWord, instruction, 12..13, 2..7),
        }
    }
}

/// A RISC-V CSS-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CSSType {
    /// The source register.
    pub rs2: u8,
    /// The funct3 field.
    pub funct3: u8,
    /// The immediate.
    pub imm: HalfWord,
}

impl CSSType {
    /// Decodes a [CSSType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rs2: bits!(u8, instruction, 2..7),
            funct3: bits!(u8, instruction, 13..16),
            imm: bits!(HalfWord, instruction, 7..13),
        }
    }
}

/// A RISC-V CIW-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CIWType {
    /// The destination register.
    pub rd: u8,
    /// The funct3 field.
    pub funct3: u8,
    /// The immediate.
    pub imm: HalfWord,
}

impl CIWType {
    /// Decodes a [CIWType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rd: bits!(u8, instruction, 2..5),
            funct3: bits!(u8, instruction, 13..16),
            imm: bits!(HalfWord, instruction, 5..13),
        }
    }
}

/// A RISC-V CL-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CLType {
    /// The destination register.
    pub rd: u8,
    /// The source register.
    pub rs1: u8,
    /// The funct3 field.
    pub funct3: u8,
    /// The immediate.
    pub imm: HalfWord,
}

impl CLType {
    /// Decodes a [CLType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rd: bits!(u8, instruction, 2..5),
            rs1: bits!(u8, instruction, 7..10),
            funct3: bits!(u8, instruction, 13..16),
            imm: twiddle!(HalfWord, instruction, 10..13, 5..7),
        }
    }
}

/// A RISC-V CS-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CSType {
    /// The first source register.
    pub rs1: u8,
    /// The second source register.
    pub rs2: u8,
    /// The funct3 field.
    pub funct3: u8,
    /// The immediate.
    pub imm: HalfWord,
}

impl CSType {
    /// Decodes a [CSType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rs1: bits!(u8, instruction, 7..10),
            rs2: bits!(u8, instruction, 2..5),
            funct3: bits!(u8, instruction, 13..16),
            imm: twiddle!(HalfWord, instruction, 10..13, 5..7),
        }
    }
}

/// A RISC-V CB-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CBType {
    /// The first source register.
    pub rs1: u8,
    /// The funct3 field.
    pub funct3: u8,
    /// The memory offset.
    pub offset: HalfWord,
}

impl CBType {
    /// Decodes a [CBType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self {
            rs1: bits!(u8, instruction, 7..10),
            funct3: bits!(u8, instruction, 13..16),
            offset: twiddle!(HalfWord, instruction, 10..13, 2..7),
        }
    }
}

/// A RISC-V CJ-Type instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CJType {
    /// The funct3 field.
    pub funct3: u8,
    /// The jump target.
    pub target: HalfWord,
}

impl CJType {
    /// Decodes a [CJType] instruction from a 16-bit [HalfWord].
    pub fn decode(instruction: HalfWord) -> Self {
        Self { funct3: bits!(u8, instruction, 13..16), target: bits!(HalfWord, instruction, 2..13) }
    }
}

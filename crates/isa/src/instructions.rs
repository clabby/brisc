//! RISC-V Instruction Types

use crate::{
    arch::Word, BranchFunction, EnvironmentFunction, ImmediateArithmeticFunction,
    InstructionDecodeError, LoadFunction, RegisterArithmeticFunction, StoreFunction, XWord,
};

mod b_type;
pub use b_type::BType;

mod i_type;
pub use i_type::IType;

mod j_type;
pub use j_type::JType;

mod r_type;
pub use r_type::RType;

mod s_type;
pub use s_type::SType;

mod u_type;
pub use u_type::UType;

#[cfg(feature = "c")]
mod rvc;
#[cfg(feature = "c")]
pub use rvc::*;

/// RISC-V Instructions supported by `brisc`.
///
/// Each variant of this enum represents a different RISC-V opcode. Variants contain the decoded
/// instruction and the function variant that the instruction performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Instruction {
    /// Memory load operations (RV32I)
    MemoryLoad(IType, LoadFunction) = 0b000_0011,
    /// Memory store operations (RV32I)
    MemoryStore(SType, StoreFunction) = 0b010_0011,
    /// Branching operations (RV32I)
    Branch(BType, BranchFunction) = 0b110_0011,
    /// Immediate arithmetic operations (RV32I)
    ImmediateArithmetic(IType, ImmediateArithmeticFunction) = 0b001_0011,
    /// Register arithmetic operations (RV32I)
    RegisterArithmetic(RType, RegisterArithmeticFunction) = 0b011_0011,
    /// Load upper immediate (RV32I)
    Lui(UType) = 0b011_0111,
    /// Add upper item to PC (RV32I)
    Auipc(UType) = 0b001_0111,
    /// Jump and link (RV32I)
    Jal(JType) = 0b110_1111,
    /// Jump and link register (RV32I)
    Jalr(IType) = 0b110_0111,
    /// Environment (RV32I)
    Environment(IType, EnvironmentFunction) = 0b111_0011,
    /// Fence operations (RV32I)
    Fence = 0b000_1111,
    /// Immediate arithmetic word operations (RV64I)
    #[cfg(feature = "64-bit")]
    ImmediateArithmeticWord(IType, crate::functions::ImmediateArithmeticWordFunction) = 0b001_1011,
    /// Register arithmetic word operations (RV64I)
    #[cfg(feature = "64-bit")]
    RegisterArithmeticWord(RType, crate::functions::RegisterArithmeticWordFunction) = 0b011_1011,
    /// AMO operations (RV32A)
    #[cfg(feature = "a")]
    Amo(RType, crate::functions::AmoFunction) = 0b010_1111,
}

impl Instruction {
    /// Returns the `rs1` value of the instruction, if applicable for the instruction type.
    pub const fn rs1(&self) -> Option<u8> {
        match self {
            Self::MemoryLoad(i_type, _) => Some(i_type.rs1),
            Self::MemoryStore(s_type, _) => Some(s_type.rs1),
            Self::Branch(b_type, _) => Some(b_type.rs1),
            Self::ImmediateArithmetic(i_type, _) => Some(i_type.rs1),
            Self::RegisterArithmetic(r_type, _) => Some(r_type.rs1),
            Self::Jalr(i_type) => Some(i_type.rs1),
            Self::Environment(i_type, _) => Some(i_type.rs1),
            #[cfg(feature = "64-bit")]
            Self::ImmediateArithmeticWord(i_type, _) => Some(i_type.rs1),
            #[cfg(feature = "64-bit")]
            Self::RegisterArithmeticWord(r_type, _) => Some(r_type.rs1),
            #[cfg(feature = "a")]
            Self::Amo(r_type, _) => Some(r_type.rs1),
            _ => None,
        }
    }

    /// Returns the `rs2` value of the instruction, if applicable for the instruction type.
    pub const fn rs2(&self) -> Option<u8> {
        match self {
            Self::MemoryStore(s_type, _) => Some(s_type.rs2),
            Self::Branch(b_type, _) => Some(b_type.rs2),
            Self::RegisterArithmetic(r_type, _) => Some(r_type.rs2),
            #[cfg(feature = "64-bit")]
            Self::RegisterArithmeticWord(r_type, _) => Some(r_type.rs2),
            #[cfg(feature = "a")]
            Self::Amo(r_type, _) => Some(r_type.rs2),
            _ => None,
        }
    }

    /// Returns the `rd` value of the instruction, if applicable for the instruction type.
    pub const fn rd(&self) -> Option<u8> {
        match self {
            Self::MemoryLoad(i_type, _) => Some(i_type.rd),
            Self::ImmediateArithmetic(i_type, _) => Some(i_type.rd),
            Self::RegisterArithmetic(r_type, _) => Some(r_type.rd),
            Self::Lui(u_type) => Some(u_type.rd),
            Self::Auipc(u_type) => Some(u_type.rd),
            Self::Jal(j_type) => Some(j_type.rd),
            Self::Jalr(i_type) => Some(i_type.rd),
            Self::Environment(i_type, _) => Some(i_type.rd),
            #[cfg(feature = "64-bit")]
            Self::ImmediateArithmeticWord(i_type, _) => Some(i_type.rd),
            #[cfg(feature = "64-bit")]
            Self::RegisterArithmeticWord(r_type, _) => Some(r_type.rd),
            #[cfg(feature = "a")]
            Self::Amo(r_type, _) => Some(r_type.rd),
            _ => None,
        }
    }

    /// Returns the immediate value of the instruction, if applicable for the instruction type.
    pub const fn immediate(&self) -> Option<XWord> {
        match self {
            Self::MemoryLoad(i_type, _) => Some(i_type.imm),
            Self::MemoryStore(s_type, _) => Some(s_type.imm),
            Self::Branch(b_type, _) => Some(b_type.imm),
            Self::ImmediateArithmetic(i_type, _) => Some(i_type.imm),
            Self::Lui(u_type) => Some(u_type.imm),
            Self::Auipc(u_type) => Some(u_type.imm),
            Self::Jal(j_type) => Some(j_type.imm),
            Self::Jalr(i_type) => Some(i_type.imm),
            Self::Environment(i_type, _) => Some(i_type.imm),
            #[cfg(feature = "64-bit")]
            Self::ImmediateArithmeticWord(i_type, _) => Some(i_type.imm),
            _ => None,
        }
    }

    /// Returns whether the instruction is a system call.
    pub const fn is_system_call(&self) -> bool {
        matches!(self, Self::Environment(_, EnvironmentFunction::Ecall))
    }
}

impl TryFrom<Word> for Instruction {
    type Error = InstructionDecodeError;

    fn try_from(value: Word) -> Result<Self, Self::Error> {
        // If the `c` feature is enabled, check if the function is compressed, and decode it
        // + map it to the standard instruction if so.
        #[cfg(feature = "c")]
        if is_compressed(value) {
            return CompressedInstruction::decode(value as crate::HalfWord)
                .map(CompressedInstruction::expand);
        }

        let opcode = (value & 0x7F) as u8;

        // Use a direct jump table based on opcodes for faster dispatch
        match opcode {
            0b000_0011 => {
                // Memory load operations - decode once and reuse
                let i_type = IType::decode(value);
                LoadFunction::try_from(&i_type).map(|f| Self::MemoryLoad(i_type, f))
            }
            0b010_0011 => {
                // Memory store operations
                let s_type = SType::decode(value);
                StoreFunction::try_from(&s_type).map(|f| Self::MemoryStore(s_type, f))
            }
            0b110_0011 => {
                // Branch operations
                let b_type = BType::decode(value);
                BranchFunction::try_from(&b_type).map(|f| Self::Branch(b_type, f))
            }
            0b001_0011 => {
                // Immediate arithmetic operations
                let i_type = IType::decode(value);
                ImmediateArithmeticFunction::try_from(&i_type)
                    .map(|f| Self::ImmediateArithmetic(i_type, f))
            }
            0b011_0011 => {
                // Register arithmetic operations
                let r_type = RType::decode(value);
                RegisterArithmeticFunction::try_from(&r_type)
                    .map(|f| Self::RegisterArithmetic(r_type, f))
            }
            0b011_0111 => {
                // LUI - simple U-type, no additional function decode
                Ok(Self::Lui(UType::decode(value)))
            }
            0b001_0111 => {
                // AUIPC - simple U-type, no additional function decode
                Ok(Self::Auipc(UType::decode(value)))
            }
            0b110_1111 => {
                // JAL - simple J-type, no additional function decode
                Ok(Self::Jal(JType::decode(value)))
            }
            0b110_0111 => {
                // JALR - simple I-type, no additional function decode
                Ok(Self::Jalr(IType::decode(value)))
            }
            0b111_0011 => {
                // Environment calls
                let i_type = IType::decode(value);
                EnvironmentFunction::try_from(&i_type).map(|f| Self::Environment(i_type, f))
            }
            0b000_1111 => Ok(Self::Fence),

            // Feature-gated instructions
            #[cfg(feature = "64-bit")]
            0b001_1011 => {
                let i_type = IType::decode(value);
                crate::functions::ImmediateArithmeticWordFunction::try_from(&i_type)
                    .map(|f| Self::ImmediateArithmeticWord(i_type, f))
            }
            #[cfg(feature = "64-bit")]
            0b011_1011 => {
                let r_type = RType::decode(value);
                crate::functions::RegisterArithmeticWordFunction::try_from(&r_type)
                    .map(|f| Self::RegisterArithmeticWord(r_type, f))
            }
            #[cfg(feature = "a")]
            0b010_1111 => {
                let r_type = RType::decode(value);
                crate::functions::AmoFunction::try_from(&r_type).map(|f| Self::Amo(r_type, f))
            }
            _ => Err(InstructionDecodeError::InvalidOpcode(opcode)),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::sign_extend;

    #[test]
    fn test_static_instruction_decode() {
        let raw: Word = 0b111111000000_01010_000_10101_0010011;
        let instruction = Instruction::try_from(raw).unwrap();

        assert!(matches!(instruction, Instruction::ImmediateArithmetic(_, _)));
        if let Instruction::ImmediateArithmetic(instruction, funct) = instruction {
            assert_eq!(
                instruction,
                IType {
                    rd: 0b10101,
                    funct3: 0b000,
                    rs1: 0b01010,
                    imm: sign_extend(0b111111000000, 11)
                }
            );
            assert!(matches!(funct, ImmediateArithmeticFunction::Addi));
        } else {
            panic!("Expected ImmediateArithmetic instruction");
        }
    }
}

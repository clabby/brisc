//! RISC-V I-Type instruction

use crate::{bits, Word};

/// A RISC-V I-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct IType {
    /// The destination register.
    pub rd: u8,
    /// funct3 field.
    pub funct3: u8,
    /// The source register.
    pub rs1: u8,
    /// The sign-extended immediate
    pub imm: i32,
}

impl IType {
    /// Decodes an [IType] instruction from a 32-bit [Word].
    pub const fn decode(instruction: Word) -> Self {
        Self {
            rd: bits!(u8, instruction, 7..12),
            funct3: bits!(u8, instruction, 12..15),
            rs1: bits!(u8, instruction, 15..20),
            imm: instruction as i32 >> 20,
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode_no_sign_extend() {
        let instruction = 0b010011001000_11000_010_00100_0000000;

        let itype = IType::decode(instruction);
        assert_eq!(itype.rd, 0b00100);
        assert_eq!(itype.funct3, 0b010);
        assert_eq!(itype.rs1, 0b11000);
        assert_eq!(itype.imm, 0b010011001000);
    }

    #[test]
    fn test_decode_sign_extend() {
        let instruction = 0b110011001000_11000_010_00100_0000000;

        let itype = IType::decode(instruction);
        assert_eq!(itype.rd, 0b00100);
        assert_eq!(itype.funct3, 0b010);
        assert_eq!(itype.rs1, 0b11000);
        assert_eq!(itype.imm, 0b110011001000 - (1 << 12));
    }
}

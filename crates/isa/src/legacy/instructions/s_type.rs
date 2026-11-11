//! Risc-V S-Type instruction

use crate::{bits, sign_extend, twiddle, Word, XWord};

/// A RISC-V S-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SType {
    /// funct3 field.
    pub funct3: u8,
    /// The first source register.
    pub rs1: u8,
    /// The second source register.
    pub rs2: u8,
    /// The sign-extended immediate
    pub imm: XWord,
}

impl SType {
    /// Decodes an [SType] instruction from a 32-bit [Word].
    pub fn decode(instruction: Word) -> Self {
        Self {
            funct3: bits!(u8, instruction, 12..15),
            rs1: bits!(u8, instruction, 15..20),
            rs2: bits!(u8, instruction, 20..25),
            imm: sign_extend(twiddle!(XWord, instruction, 25..32, 7..12), 11),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode_no_sign_extend() {
        let instruction = 0b0110001_00100_10101_111_11000_0000000;

        let stype = SType::decode(instruction);
        assert_eq!(stype.funct3, 0b111);
        assert_eq!(stype.rs1, 0b10101);
        assert_eq!(stype.rs2, 0b00100);
        assert_eq!(stype.imm, 0b011000111000);
    }

    #[test]
    fn test_decode_sign_extend() {
        let instruction = 0b1110001_00100_10101_111_11000_0000000;

        let stype = SType::decode(instruction);
        assert_eq!(stype.funct3, 0b111);
        assert_eq!(stype.rs1, 0b10101);
        assert_eq!(stype.rs2, 0b00100);
        assert_eq!(stype.imm, sign_extend(0b111000111000, 11));
    }
}

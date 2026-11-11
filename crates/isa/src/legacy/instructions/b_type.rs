//! Risc-V B-Type instruction

use crate::{bits, sign_extend, twiddle, Word, XWord};

/// A RISC-V B-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BType {
    /// funct3 field.
    pub funct3: u8,
    /// The first source register.
    pub rs1: u8,
    /// The second source register.
    pub rs2: u8,
    /// The sign-extended immediate
    pub imm: XWord,
}

impl BType {
    /// Decodes an [BType] instruction from a 32-bit [Word].
    pub fn decode(instruction: Word) -> Self {
        Self {
            funct3: bits!(u8, instruction, 12..15),
            rs1: bits!(u8, instruction, 15..20),
            rs2: bits!(u8, instruction, 20..25),
            imm: sign_extend(twiddle!(XWord, instruction, 31..32, 7..8, 25..31, 8..12) << 1, 12),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode_no_sign_extend() {
        let instruction = 0b0_010110_11111_01010_101_0010_1_0000000;

        let btype = BType::decode(instruction);
        assert_eq!(btype.funct3, 0b101);
        assert_eq!(btype.rs1, 0b01010);
        assert_eq!(btype.rs2, 0b11111);
        assert_eq!(btype.imm, 0b0_1_010110_0010 << 1);
    }

    #[test]
    fn test_decode_sign_extend() {
        let instruction = 0b1_010110_11111_01010_101_0010_1_0000000;

        let btype = BType::decode(instruction);
        assert_eq!(btype.funct3, 0b101);
        assert_eq!(btype.rs1, 0b01010);
        assert_eq!(btype.rs2, 0b11111);
        assert_eq!(btype.imm, sign_extend(0b1_1_010110_0010 << 1, 12));
    }
}

//! RISC-V J-Type instruction

use crate::{bits, Word};

/// A RISC-V J-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct JType {
    /// The destination register.
    pub rd: u8,
    /// The sign-extended immediate
    pub imm: i32,
}

impl JType {
    /// Decodes an [JType] instruction from a 32-bit [Word].
    pub const fn decode(instruction: Word) -> Self {
        Self {
            rd: bits!(u8, instruction, 7..12),
            // imm[20|10:1|11|19:12] | rd | opcode
            imm: (instruction as i32 >> 31 << 20) |
                (bits!(i32, instruction, 12..20) << 12) |
                (bits!(i32, instruction, 20..21) << 11) |
                (bits!(i32, instruction, 21..31) << 1),
        }
    }

    /// Encodes the fields as a 32-bit [Word] whose opcode, the low seven bits, is zero.
    ///
    /// A field that is too large for its place in the word loses its upper bits, and the
    /// immediate its lowest bit.
    pub const fn encode(&self) -> Word {
        let imm = self.imm as Word;
        (self.rd as Word & 0x1f) << 7 |
            bits!(Word, imm, 12..20) << 12 |
            bits!(Word, imm, 11..12) << 20 |
            bits!(Word, imm, 1..11) << 21 |
            bits!(Word, imm, 20..21) << 31
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode_no_sign_extend() {
        let instruction = 0b0_0101010101_1_10000000_11111_0000000;

        let jtype = JType::decode(instruction);
        assert_eq!(jtype.rd, 0b11111);
        assert_eq!(jtype.imm, 0b0_10000000_1_0101010101 << 1);
    }

    #[test]
    fn test_decode_sign_extend() {
        let instruction = 0b1_0101010101_1_00000000_11111_0000000;

        let jtype = JType::decode(instruction);
        assert_eq!(jtype.rd, 0b11111);
        assert_eq!(jtype.imm, (0b1_00000000_1_0101010101 << 1) - (1 << 21));
    }
}

//! RISC-V B-Type instruction

use crate::{bits, Word};

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
    pub imm: i32,
}

impl BType {
    /// Decodes an [BType] instruction from a 32-bit [Word].
    pub const fn decode(instruction: Word) -> Self {
        Self {
            funct3: bits!(u8, instruction, 12..15),
            rs1: bits!(u8, instruction, 15..20),
            rs2: bits!(u8, instruction, 20..25),
            // imm[12|10:5] | rs2 | rs1 | funct3 | imm[4:1|11] | opcode
            imm: (instruction as i32 >> 31 << 12) |
                (bits!(i32, instruction, 7..8) << 11) |
                (bits!(i32, instruction, 25..31) << 5) |
                (bits!(i32, instruction, 8..12) << 1),
        }
    }

    /// Encodes the fields as a 32-bit [Word] whose opcode, the low seven bits, is zero.
    ///
    /// A field that is too large for its place in the word loses its upper bits, and the
    /// immediate its lowest bit.
    pub const fn encode(&self) -> Word {
        let imm = self.imm as Word;
        bits!(Word, imm, 11..12) << 7 |
            bits!(Word, imm, 1..5) << 8 |
            (self.funct3 as Word & 0x7) << 12 |
            (self.rs1 as Word & 0x1f) << 15 |
            (self.rs2 as Word & 0x1f) << 20 |
            bits!(Word, imm, 5..11) << 25 |
            bits!(Word, imm, 12..13) << 31
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
        assert_eq!(btype.imm, (0b1_1_010110_0010 << 1) - (1 << 13));
    }
}

//! RISC-V R-Type instruction

use crate::{bits, Word};

/// A RISC-V R-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RType {
    /// The destination register.
    pub rd: u8,
    /// funct3 field.
    pub funct3: u8,
    /// The source register (1).
    pub rs1: u8,
    /// The source register (2).
    pub rs2: u8,
    /// funct7 field.
    pub funct7: u8,
}

impl RType {
    /// Decodes an [`RType`] instruction from a 32-bit [`Word`].
    pub const fn decode(instruction: Word) -> Self {
        Self {
            rd: bits!(u8, instruction, 7..12),
            funct3: bits!(u8, instruction, 12..15),
            rs1: bits!(u8, instruction, 15..20),
            rs2: bits!(u8, instruction, 20..25),
            funct7: bits!(u8, instruction, 25..32),
        }
    }

    /// Encodes the fields as a 32-bit [`Word`] whose opcode, the low seven bits, is zero.
    ///
    /// A field that is too large for its place in the word loses its upper bits.
    pub const fn encode(&self) -> Word {
        (self.rd as Word & 0x1f) << 7 |
            (self.funct3 as Word & 0x7) << 12 |
            (self.rs1 as Word & 0x1f) << 15 |
            (self.rs2 as Word & 0x1f) << 20 |
            (self.funct7 as Word & 0x7f) << 25
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode() {
        let instruction = 0b1100110_01000_11000_010_00100_0000000;

        let rtype = RType::decode(instruction);
        assert_eq!(rtype.rd, 0b00100);
        assert_eq!(rtype.funct3, 0b010);
        assert_eq!(rtype.rs1, 0b11000);
        assert_eq!(rtype.rs2, 0b01000);
        assert_eq!(rtype.funct7, 0b1100110);
    }
}

//! Risc-V U-Type instruction

use crate::{arch::Word, bits, sign_extend, XWord};

/// A RISC-V U-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UType {
    /// The destination register.
    pub rd: u8,
    /// The sign-extended immediate
    pub imm: XWord,
}

impl UType {
    /// Decodes an [UType] instruction from a 32-bit [Word].
    pub fn decode(instruction: Word) -> Self {
        Self {
            rd: bits!(u8, instruction, 7..12),
            imm: sign_extend(bits!(XWord, instruction, 12..32) << 12, 31),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_decode_no_sign_extend() {
        let instruction = 0b01000100100100010000_01010_1111111;

        let utype = UType::decode(instruction);
        assert_eq!(utype.rd, 0b01010);
        assert_eq!(utype.imm, 0b01000100100100010000 << 12);
    }

    #[test]
    fn test_decode_sign_extend() {
        let instruction = 0b10000100100100010000_01010_1111111;

        let utype = UType::decode(instruction);
        assert_eq!(utype.rd, 0b01010);
        assert_eq!(utype.imm, sign_extend(0b10000100100100010000 << 12, 31));
    }
}

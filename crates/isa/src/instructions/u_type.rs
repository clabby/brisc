//! RISC-V U-Type instruction

use crate::{bits, Word};

/// A RISC-V U-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UType {
    /// The destination register.
    pub rd: u8,
    /// The sign-extended immediate
    pub imm: i32,
}

impl UType {
    /// Decodes an [UType] instruction from a 32-bit [Word].
    pub const fn decode(instruction: Word) -> Self {
        Self { rd: bits!(u8, instruction, 7..12), imm: (instruction & 0xffff_f000) as i32 }
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
        assert_eq!(utype.imm as i64, (0b10000100100100010000 << 12) - (1 << 32));
    }
}

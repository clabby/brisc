//! Risc-V J-Type instruction

use crate::{arch::Word, bits, sign_extend, twiddle, XWord};

/// A RISC-V J-Type instruction.
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct JType {
    /// The destination register.
    pub rd: u8,
    /// The sign-extended immediate
    pub imm: XWord,
}

impl JType {
    /// Decodes an [JType] instruction from a 32-bit [Word].
    pub fn decode(instruction: Word) -> Self {
        Self {
            rd: bits!(u8, instruction, 7..12),
            imm: sign_extend(twiddle!(XWord, instruction, 31..32, 12..20, 20..21, 21..31) << 1, 20),
        }
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
        assert_eq!(jtype.imm, sign_extend(0b1_00000000_1_0101010101 << 1, 20));
    }
}

//! RISC-V instructions and their decoding.

use crate::{InstructionDecodeError, Opcode, Word, X_LEN};

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

/// Returns `true` if the instruction that starts with `parcel` is 16 bits long, which is the case
/// unless the two least significant bits are both set.
///
/// Only the low 16 bits of `parcel` are looked at, so it can be the first half of an instruction
/// or a word with the instruction in its low bits.
pub const fn is_compressed(parcel: Word) -> bool {
    parcel & 0b11 != 0b11
}

/// A decoded instruction: an [Opcode] with its operands.
///
/// A 16-bit instruction of the C extension decodes to the same opcode and operands as the 32-bit
/// instruction it abbreviates; only [Instruction::length] tells the two apart.
///
/// An operand that an instruction does not have is zero, so two instructions are equal exactly
/// when they have the same encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Instruction {
    /// The instruction.
    pub opcode: Opcode,
    /// The destination register.
    pub rd: u8,
    /// The first source register.
    ///
    /// For [Opcode::Csrrwi], [Opcode::Csrrsi] and [Opcode::Csrrci] this is not a register but the
    /// 5-bit immediate that is written to the CSR.
    pub rs1: u8,
    /// The second source register.
    pub rs2: u8,
    /// The immediate, sign-extended. Every immediate of the supported instruction sets fits in
    /// 32 bits.
    ///
    /// - Loads, stores, branches, [Opcode::Jal] and [Opcode::Jalr]: the offset in bytes.
    /// - [Opcode::Lui] and [Opcode::Auipc]: the value the instruction loads or adds, a multiple of
    ///   4096.
    /// - Shifts by an immediate: the shift amount.
    /// - CSR instructions: the address of the CSR, from 0 to 4095.
    /// - Instructions of the A extension: the ordering constraints, [Instruction::AQ] and
    ///   [Instruction::RL].
    /// - [Opcode::Fence] and [Opcode::FenceI]: the 12 bits above the `rs1` field, which hold the
    ///   predecessor and successor sets of a `fence`.
    pub imm: i32,
    /// The length of the instruction in bytes: 2 or 4.
    pub length: u8,
}

impl Instruction {
    /// The bit of [Instruction::imm] that is the `aq` bit of an instruction of the A extension.
    pub const AQ: i32 = 0b10;

    /// The bit of [Instruction::imm] that is the `rl` bit of an instruction of the A extension.
    pub const RL: i32 = 0b01;

    /// Decodes the instruction in the low bits of `word`.
    ///
    /// If the instruction is 16 bits long, the upper half of `word` is ignored.
    ///
    /// Decoding is exact: a word is an instruction only if the instruction sets selected by the
    /// crate features define it. Encodings that are reserved, or that belong to an extension
    /// that is not enabled, are errors. Hints, which are encodings of an instruction that only
    /// writes `x0`, decode as that instruction.
    pub const fn decode(word: Word) -> Result<Self, InstructionDecodeError> {
        use Opcode::*;

        #[cfg(feature = "c")]
        if is_compressed(word) {
            return match CompressedInstruction::decode(word as crate::HalfWord) {
                Ok(compressed) => Ok(compressed.expand()),
                Err(error) => Err(error),
            };
        }

        let major = (word & 0x7f) as u8;
        Ok(match major {
            // LOAD
            0b000_0011 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => Lb,
                    0b001 => Lh,
                    0b010 => Lw,
                    0b100 => Lbu,
                    0b101 => Lhu,
                    #[cfg(feature = "64-bit")]
                    0b110 => Lwu,
                    #[cfg(feature = "64-bit")]
                    0b011 => Ld,
                    _ => return Err(invalid_function(i.funct3, 0)),
                };
                Self::from_i(opcode, i)
            }
            // MISC-MEM
            0b000_1111 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => Fence,
                    0b001 => FenceI,
                    _ => return Err(invalid_function(i.funct3, 0)),
                };
                Self::from_i(opcode, i)
            }
            // OP-IMM
            0b001_0011 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => Addi,
                    0b010 => Slti,
                    0b011 => Sltiu,
                    0b100 => Xori,
                    0b110 => Ori,
                    0b111 => Andi,
                    _ => return Self::decode_shift(i, X_LEN as i32, Slli, Srli, Srai),
                };
                Self::from_i(opcode, i)
            }
            0b001_0111 => Self::from_u(Auipc, UType::decode(word)),
            // OP-IMM-32
            #[cfg(feature = "64-bit")]
            0b001_1011 => {
                let i = IType::decode(word);
                match i.funct3 {
                    0b000 => Self::from_i(Addiw, i),
                    _ => return Self::decode_shift(i, 32, Slliw, Srliw, Sraiw),
                }
            }
            // STORE
            0b010_0011 => {
                let s = SType::decode(word);
                let opcode = match s.funct3 {
                    0b000 => Sb,
                    0b001 => Sh,
                    0b010 => Sw,
                    #[cfg(feature = "64-bit")]
                    0b011 => Sd,
                    _ => return Err(invalid_function(s.funct3, 0)),
                };
                Self { opcode, rd: 0, rs1: s.rs1, rs2: s.rs2, imm: s.imm, length: 4 }
            }
            // AMO
            #[cfg(feature = "a")]
            0b010_1111 => {
                let r = RType::decode(word);
                // The function is in the upper five bits of funct7, above the ordering bits.
                let opcode = match (r.funct3, r.funct7 >> 2) {
                    (0b010, 0b00010) => LrW,
                    (0b010, 0b00011) => ScW,
                    (0b010, 0b00001) => AmoswapW,
                    (0b010, 0b00000) => AmoaddW,
                    (0b010, 0b00100) => AmoxorW,
                    (0b010, 0b01100) => AmoandW,
                    (0b010, 0b01000) => AmoorW,
                    (0b010, 0b10000) => AmominW,
                    (0b010, 0b10100) => AmomaxW,
                    (0b010, 0b11000) => AmominuW,
                    (0b010, 0b11100) => AmomaxuW,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00010) => LrD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00011) => ScD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00001) => AmoswapD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00000) => AmoaddD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b00100) => AmoxorD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b01100) => AmoandD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b01000) => AmoorD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b10000) => AmominD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b10100) => AmomaxD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b11000) => AmominuD,
                    #[cfg(feature = "64-bit")]
                    (0b011, 0b11100) => AmomaxuD,
                    _ => return Err(invalid_function(r.funct3, r.funct7)),
                };
                // A load-reserved has no second source register.
                if matches!(opcode, LrW | LrD) && r.rs2 != 0 {
                    return Err(InstructionDecodeError::Reserved);
                }
                Self { imm: (r.funct7 & 0b11) as i32, ..Self::from_r(opcode, r) }
            }
            // OP
            0b011_0011 => {
                let r = RType::decode(word);
                let opcode = match (r.funct7, r.funct3) {
                    (0x00, 0b000) => Add,
                    (0x20, 0b000) => Sub,
                    (0x00, 0b001) => Sll,
                    (0x00, 0b010) => Slt,
                    (0x00, 0b011) => Sltu,
                    (0x00, 0b100) => Xor,
                    (0x00, 0b101) => Srl,
                    (0x20, 0b101) => Sra,
                    (0x00, 0b110) => Or,
                    (0x00, 0b111) => And,
                    #[cfg(feature = "m")]
                    (0x01, 0b000) => Mul,
                    #[cfg(feature = "m")]
                    (0x01, 0b001) => Mulh,
                    #[cfg(feature = "m")]
                    (0x01, 0b010) => Mulhsu,
                    #[cfg(feature = "m")]
                    (0x01, 0b011) => Mulhu,
                    #[cfg(feature = "m")]
                    (0x01, 0b100) => Div,
                    #[cfg(feature = "m")]
                    (0x01, 0b101) => Divu,
                    #[cfg(feature = "m")]
                    (0x01, 0b110) => Rem,
                    #[cfg(feature = "m")]
                    (0x01, 0b111) => Remu,
                    _ => return Err(invalid_function(r.funct3, r.funct7)),
                };
                Self::from_r(opcode, r)
            }
            0b011_0111 => Self::from_u(Lui, UType::decode(word)),
            // OP-32
            #[cfg(feature = "64-bit")]
            0b011_1011 => {
                let r = RType::decode(word);
                let opcode = match (r.funct7, r.funct3) {
                    (0x00, 0b000) => Addw,
                    (0x20, 0b000) => Subw,
                    (0x00, 0b001) => Sllw,
                    (0x00, 0b101) => Srlw,
                    (0x20, 0b101) => Sraw,
                    #[cfg(feature = "m")]
                    (0x01, 0b000) => Mulw,
                    #[cfg(feature = "m")]
                    (0x01, 0b100) => Divw,
                    #[cfg(feature = "m")]
                    (0x01, 0b101) => Divuw,
                    #[cfg(feature = "m")]
                    (0x01, 0b110) => Remw,
                    #[cfg(feature = "m")]
                    (0x01, 0b111) => Remuw,
                    _ => return Err(invalid_function(r.funct3, r.funct7)),
                };
                Self::from_r(opcode, r)
            }
            // BRANCH
            0b110_0011 => {
                let b = BType::decode(word);
                let opcode = match b.funct3 {
                    0b000 => Beq,
                    0b001 => Bne,
                    0b100 => Blt,
                    0b101 => Bge,
                    0b110 => Bltu,
                    0b111 => Bgeu,
                    _ => return Err(invalid_function(b.funct3, 0)),
                };
                Self { opcode, rd: 0, rs1: b.rs1, rs2: b.rs2, imm: b.imm, length: 4 }
            }
            0b110_0111 => {
                let i = IType::decode(word);
                if i.funct3 != 0 {
                    return Err(invalid_function(i.funct3, 0));
                }
                Self::from_i(Jalr, i)
            }
            0b110_1111 => {
                let j = JType::decode(word);
                Self { opcode: Jal, rd: j.rd, rs1: 0, rs2: 0, imm: j.imm, length: 4 }
            }
            // SYSTEM
            0b111_0011 => {
                let i = IType::decode(word);
                let opcode = match i.funct3 {
                    0b000 => return Self::decode_privileged(word),
                    0b001 => Csrrw,
                    0b010 => Csrrs,
                    0b011 => Csrrc,
                    0b101 => Csrrwi,
                    0b110 => Csrrsi,
                    0b111 => Csrrci,
                    _ => return Err(invalid_function(i.funct3, 0)),
                };
                // The address of a CSR is not sign-extended.
                Self { imm: (word >> 20) as i32, ..Self::from_i(opcode, i) }
            }
            _ => return Err(InstructionDecodeError::InvalidOpcode(major)),
        })
    }

    /// Decodes a shift by an immediate of a value that is `width` bits wide.
    ///
    /// The low bits of the immediate are the shift amount, as many as it takes to index a bit of
    /// the value. The bits above them select between a logical and an arithmetic shift.
    const fn decode_shift(
        i: IType,
        width: i32,
        left: Opcode,
        right_logical: Opcode,
        right_arithmetic: Opcode,
    ) -> Result<Self, InstructionDecodeError> {
        let shamt = i.imm & (width - 1);
        let function = (i.imm & 0xfff) ^ shamt;
        let opcode = match (i.funct3, function) {
            (0b001, 0x000) => left,
            (0b101, 0x000) => right_logical,
            (0b101, 0x400) => right_arithmetic,
            _ => return Err(invalid_function(i.funct3, (function >> 5) as u8)),
        };
        Ok(Self { imm: shamt, ..Self::from_i(opcode, i) })
    }

    /// Decodes an instruction with the SYSTEM major opcode and a funct3 of zero. Of these only
    /// `sfence.vma` has operands.
    const fn decode_privileged(word: Word) -> Result<Self, InstructionDecodeError> {
        let opcode = match word {
            0x0000_0073 => Opcode::Ecall,
            0x0010_0073 => Opcode::Ebreak,
            0x1020_0073 => Opcode::Sret,
            0x3020_0073 => Opcode::Mret,
            0x1050_0073 => Opcode::Wfi,
            _ => {
                let r = RType::decode(word);
                if r.funct7 != 0b000_1001 || r.rd != 0 {
                    return Err(invalid_function(r.funct3, r.funct7));
                }
                return Ok(Self::from_r(Opcode::SfenceVma, r));
            }
        };
        Ok(Self { opcode, rd: 0, rs1: 0, rs2: 0, imm: 0, length: 4 })
    }

    const fn from_r(opcode: Opcode, r: RType) -> Self {
        Self { opcode, rd: r.rd, rs1: r.rs1, rs2: r.rs2, imm: 0, length: 4 }
    }

    const fn from_i(opcode: Opcode, i: IType) -> Self {
        Self { opcode, rd: i.rd, rs1: i.rs1, rs2: 0, imm: i.imm, length: 4 }
    }

    const fn from_u(opcode: Opcode, u: UType) -> Self {
        Self { opcode, rd: u.rd, rs1: 0, rs2: 0, imm: u.imm, length: 4 }
    }
}

impl TryFrom<Word> for Instruction {
    type Error = InstructionDecodeError;

    fn try_from(word: Word) -> Result<Self, Self::Error> {
        Self::decode(word)
    }
}

const fn invalid_function(q_a: u8, q_b: u8) -> InstructionDecodeError {
    InstructionDecodeError::InvalidFunction { q_a, q_b }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn all_zero_parcels_are_illegal() {
        assert!(Instruction::decode(0x0000).is_err());
        assert!(Instruction::decode(0x0000_0000).is_err());

        // A zero immediate is reserved for `c.addi4spn` whichever register it names.
        #[cfg(feature = "c")]
        assert!(Instruction::decode(0b000_00000000_001_00).is_err());
        #[cfg(feature = "c")]
        assert!(Instruction::decode(0b000_00000001_001_00).is_ok());
    }
}

//! Decoded instructions.

use super::Fault;
use brisc_isa::{
    BranchFunction, EnvironmentFunction, ImmediateArithmeticFunction, Instruction, LoadFunction,
    RegisterArithmeticFunction, StoreFunction, Word, XWord,
};

#[cfg(feature = "64-bit")]
use brisc_isa::{ImmediateArithmeticWordFunction, RegisterArithmeticWordFunction};

#[cfg(feature = "a")]
use brisc_isa::AmoFunction;

/// The bit of [Op::code] that is set when the instruction is four bytes long and clear when it is
/// two bytes long.
pub(super) const WIDE: u8 = 0x80;

/// The register slot that receives writes to `x0`.
pub(super) const SINK: u8 = 32;

/// An instruction in the form the interpreter executes.
///
/// The length of the instruction is part of [Op::code], so that every arm of the interpreter's
/// dispatch knows statically how far to advance and the address of the next [Op] never depends
/// on the contents of the current one.
#[derive(Debug, Clone, Copy)]
#[repr(C, align(8))]
pub(super) struct Op {
    /// One of the [narrow] constants, or the [wide] constant of the same name.
    pub(super) code: u8,
    /// The destination register, with `x0` replaced by [SINK].
    pub(super) rd: u8,
    /// The first source register.
    pub(super) rs1: u8,
    /// The second source register.
    pub(super) rs2: u8,
    /// The sign-extended immediate. Every immediate of the base ISA and the M, A and C extensions
    /// fits in 32 bits.
    pub(super) imm: i32,
}

impl Op {
    /// The initial content of a slot: leaves the interpreter to have the slot decoded.
    pub(super) const DECODE: Self = Self::bare(narrow::DECODE);

    /// Fills the slots past the last instruction of a page: leaves the interpreter to continue
    /// on the next page.
    pub(super) const PAGE_END: Self = Self::bare(narrow::PAGE_END);

    /// Stands in for a word that is not an instruction.
    pub(super) const ILLEGAL: Self = Self::bare(narrow::ILLEGAL);

    const fn bare(code: u8) -> Self {
        Self { code, rd: 0, rs1: 0, rs2: 0, imm: 0 }
    }
}

/// Defines the opcodes of [Op]: a constant per name in [narrow] and the same value with [WIDE]
/// set in [wide].
macro_rules! opcodes {
    ($($(#[$cfg:meta])* $name:ident),* $(,)?) => {
        /// Numbers the opcodes. Its variant names are the names of the opcode constants.
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        #[repr(u8)]
        enum Number { $($(#[$cfg])* $name),* }

        /// Opcodes of two-byte instructions, and of the opcodes that are not instructions.
        pub(super) mod narrow {
            $($(#[$cfg])* pub(in super::super) const $name: u8 = super::Number::$name as u8;)*
        }

        /// Opcodes of four-byte instructions.
        pub(super) mod wide {
            $($(#[$cfg])* pub(in super::super) const $name: u8 = super::narrow::$name | super::WIDE;)*
        }
    };
}

opcodes! {
    // Must be first: a zeroed slot asks to be decoded.
    DECODE,
    PAGE_END,
    ILLEGAL,
    LI,
    AUIPC,
    ADD,
    SUB,
    XOR,
    OR,
    AND,
    SLL,
    SRL,
    SRA,
    SLT,
    SLTU,
    ADDI,
    XORI,
    ORI,
    ANDI,
    SLLI,
    SRLI,
    SRAI,
    SLTI,
    SLTIU,
    LB,
    LH,
    LW,
    LBU,
    LHU,
    SB,
    SH,
    SW,
    BEQ,
    BNE,
    BLT,
    BGE,
    BLTU,
    BGEU,
    JAL,
    JALR,
    ECALL,
    #[cfg(feature = "64-bit")]
    LWU,
    #[cfg(feature = "64-bit")]
    LD,
    #[cfg(feature = "64-bit")]
    SD,
    #[cfg(feature = "64-bit")]
    ADDW,
    #[cfg(feature = "64-bit")]
    SUBW,
    #[cfg(feature = "64-bit")]
    SLLW,
    #[cfg(feature = "64-bit")]
    SRLW,
    #[cfg(feature = "64-bit")]
    SRAW,
    #[cfg(feature = "64-bit")]
    ADDIW,
    #[cfg(feature = "64-bit")]
    SLLIW,
    #[cfg(feature = "64-bit")]
    SRLIW,
    #[cfg(feature = "64-bit")]
    SRAIW,
    #[cfg(feature = "m")]
    MUL,
    #[cfg(feature = "m")]
    MULH,
    #[cfg(feature = "m")]
    MULHSU,
    #[cfg(feature = "m")]
    MULHU,
    #[cfg(feature = "m")]
    DIV,
    #[cfg(feature = "m")]
    DIVU,
    #[cfg(feature = "m")]
    REM,
    #[cfg(feature = "m")]
    REMU,
    #[cfg(all(feature = "64-bit", feature = "m"))]
    MULW,
    #[cfg(all(feature = "64-bit", feature = "m"))]
    DIVW,
    #[cfg(all(feature = "64-bit", feature = "m"))]
    DIVUW,
    #[cfg(all(feature = "64-bit", feature = "m"))]
    REMW,
    #[cfg(all(feature = "64-bit", feature = "m"))]
    REMUW,
    #[cfg(feature = "a")]
    LR,
    #[cfg(feature = "a")]
    SC,
    #[cfg(feature = "a")]
    AMOSWAP,
    #[cfg(feature = "a")]
    AMOADD,
    #[cfg(feature = "a")]
    AMOXOR,
    #[cfg(feature = "a")]
    AMOAND,
    #[cfg(feature = "a")]
    AMOOR,
    #[cfg(feature = "a")]
    AMOMIN,
    #[cfg(feature = "a")]
    AMOMAX,
    #[cfg(feature = "a")]
    AMOMINU,
    #[cfg(feature = "a")]
    AMOMAXU,
}

/// Decodes the instruction in the low bits of `raw`.
pub(super) fn lower(raw: Word) -> Result<Op, Fault> {
    let instruction = Instruction::try_from(raw)?;

    // An instruction is two bytes long unless its two low bits are set.
    let length = if raw & 0b11 == 0b11 { WIDE } else { 0 };
    let op = |code: u8, rd: u8, rs1: u8, rs2: u8, imm: XWord| Op {
        code: code | length,
        rd: if rd == 0 { SINK } else { rd },
        rs1,
        rs2,
        imm: imm as i32,
    };

    Ok(match instruction {
        Instruction::MemoryLoad(i, function) => {
            let code = match function {
                LoadFunction::Lb => narrow::LB,
                LoadFunction::Lh => narrow::LH,
                LoadFunction::Lw => narrow::LW,
                LoadFunction::Lbu => narrow::LBU,
                LoadFunction::Lhu => narrow::LHU,
                #[cfg(feature = "64-bit")]
                LoadFunction::Lwu => narrow::LWU,
                #[cfg(feature = "64-bit")]
                LoadFunction::Ld => narrow::LD,
            };
            op(code, i.rd, i.rs1, 0, i.imm)
        }
        Instruction::MemoryStore(s, function) => {
            let code = match function {
                StoreFunction::Sb => narrow::SB,
                StoreFunction::Sh => narrow::SH,
                StoreFunction::Sw => narrow::SW,
                #[cfg(feature = "64-bit")]
                StoreFunction::Sd => narrow::SD,
            };
            op(code, 0, s.rs1, s.rs2, s.imm)
        }
        Instruction::Branch(b, function) => {
            let code = match function {
                BranchFunction::Beq => narrow::BEQ,
                BranchFunction::Bne => narrow::BNE,
                BranchFunction::Blt => narrow::BLT,
                BranchFunction::Bge => narrow::BGE,
                BranchFunction::Bltu => narrow::BLTU,
                BranchFunction::Bgeu => narrow::BGEU,
            };
            op(code, 0, b.rs1, b.rs2, b.imm)
        }
        Instruction::ImmediateArithmetic(i, function) => {
            let code = match function {
                ImmediateArithmeticFunction::Addi => narrow::ADDI,
                ImmediateArithmeticFunction::Xori => narrow::XORI,
                ImmediateArithmeticFunction::Ori => narrow::ORI,
                ImmediateArithmeticFunction::Andi => narrow::ANDI,
                ImmediateArithmeticFunction::Slli => narrow::SLLI,
                ImmediateArithmeticFunction::Srli => narrow::SRLI,
                ImmediateArithmeticFunction::Srai => narrow::SRAI,
                ImmediateArithmeticFunction::Slti => narrow::SLTI,
                ImmediateArithmeticFunction::Sltiu => narrow::SLTIU,
            };
            op(code, i.rd, i.rs1, 0, i.imm)
        }
        Instruction::RegisterArithmetic(r, function) => {
            let code = match function {
                RegisterArithmeticFunction::Add => narrow::ADD,
                RegisterArithmeticFunction::Sub => narrow::SUB,
                RegisterArithmeticFunction::Xor => narrow::XOR,
                RegisterArithmeticFunction::Or => narrow::OR,
                RegisterArithmeticFunction::And => narrow::AND,
                RegisterArithmeticFunction::Sll => narrow::SLL,
                RegisterArithmeticFunction::Srl => narrow::SRL,
                RegisterArithmeticFunction::Sra => narrow::SRA,
                RegisterArithmeticFunction::Slt => narrow::SLT,
                RegisterArithmeticFunction::Sltu => narrow::SLTU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mul => narrow::MUL,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulh => narrow::MULH,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulhsu => narrow::MULHSU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Mulhu => narrow::MULHU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Div => narrow::DIV,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Divu => narrow::DIVU,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Rem => narrow::REM,
                #[cfg(feature = "m")]
                RegisterArithmeticFunction::Remu => narrow::REMU,
            };
            op(code, r.rd, r.rs1, r.rs2, 0)
        }
        Instruction::Lui(u) => op(narrow::LI, u.rd, 0, 0, u.imm),
        Instruction::Auipc(u) => op(narrow::AUIPC, u.rd, 0, 0, u.imm),
        Instruction::Jal(j) => op(narrow::JAL, j.rd, 0, 0, j.imm),
        Instruction::Jalr(i) => op(narrow::JALR, i.rd, i.rs1, 0, i.imm),
        Instruction::Environment(_, EnvironmentFunction::Ecall) => op(narrow::ECALL, 0, 0, 0, 0),
        // Breakpoints and control and status registers are not modelled: the instruction only
        // clears its destination register.
        Instruction::Environment(i, EnvironmentFunction::Ebreak) => op(narrow::LI, i.rd, 0, 0, 0),
        // With a single hart and decoded code kept coherent with memory, there is nothing to
        // order or flush.
        Instruction::Fence => op(narrow::LI, 0, 0, 0, 0),
        #[cfg(feature = "64-bit")]
        Instruction::ImmediateArithmeticWord(i, function) => {
            let code = match function {
                ImmediateArithmeticWordFunction::Addiw => narrow::ADDIW,
                ImmediateArithmeticWordFunction::Slliw => narrow::SLLIW,
                ImmediateArithmeticWordFunction::Srliw => narrow::SRLIW,
                ImmediateArithmeticWordFunction::Sraiw => narrow::SRAIW,
            };
            op(code, i.rd, i.rs1, 0, i.imm)
        }
        #[cfg(feature = "64-bit")]
        Instruction::RegisterArithmeticWord(r, function) => {
            let code = match function {
                RegisterArithmeticWordFunction::Addw => narrow::ADDW,
                RegisterArithmeticWordFunction::Subw => narrow::SUBW,
                RegisterArithmeticWordFunction::Sllw => narrow::SLLW,
                RegisterArithmeticWordFunction::Srlw => narrow::SRLW,
                RegisterArithmeticWordFunction::Sraw => narrow::SRAW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Mulw => narrow::MULW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Divw => narrow::DIVW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Divuw => narrow::DIVUW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Remw => narrow::REMW,
                #[cfg(feature = "m")]
                RegisterArithmeticWordFunction::Remuw => narrow::REMUW,
            };
            op(code, r.rd, r.rs1, r.rs2, 0)
        }
        #[cfg(feature = "a")]
        Instruction::Amo(r, function) => {
            // The immediate carries the access size in bytes.
            let size = 1u8 << r.funct3;
            if size != 4 && !(cfg!(feature = "64-bit") && size == 8) {
                return Err(Fault::BadAmoSize(size));
            }
            let code = match function {
                AmoFunction::Lr => narrow::LR,
                AmoFunction::Sc => narrow::SC,
                AmoFunction::Amoswap => narrow::AMOSWAP,
                AmoFunction::Amoadd => narrow::AMOADD,
                AmoFunction::Amoxor => narrow::AMOXOR,
                AmoFunction::Amoand => narrow::AMOAND,
                AmoFunction::Amoor => narrow::AMOOR,
                AmoFunction::Amomin => narrow::AMOMIN,
                AmoFunction::Amomax => narrow::AMOMAX,
                AmoFunction::Amominu => narrow::AMOMINU,
                AmoFunction::Amomaxu => narrow::AMOMAXU,
            };
            op(code, r.rd, r.rs1, r.rs2, size as XWord)
        }
    })
}

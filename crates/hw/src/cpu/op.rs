//! Decoded instructions.

/// The bit of [Op::code] that is set when the instruction is four bytes long and clear when it is
/// two bytes long.
pub(super) const WIDE: u8 = 0x80;

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
    /// The destination register, with `x0` replaced by [Registers::SINK].
    ///
    /// [Registers::SINK]: super::Registers::SINK
    pub(super) rd: u8,
    /// The first source register.
    pub(super) rs1: u8,
    /// The second source register.
    pub(super) rs2: u8,
    /// The sign-extended immediate. Every immediate of the base ISA and the M, A and C extensions
    /// fits in 32 bits.
    pub(super) imm: i32,
}

// Eight bytes with eight-byte alignment: a slot never straddles two cache lines.
const _: () = assert!(size_of::<Op>() == 8 && align_of::<Op>() == 8);

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

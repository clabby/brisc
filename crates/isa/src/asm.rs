//! Assembly of single instructions, for the [rv!](crate::rv) and [rv_code!](crate::rv_code)
//! macros.

use crate::{register_index, Format, Instruction, Opcode, Word};

/// An operand of an instruction as assembly language writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operand {
    /// An integer register.
    Register(u8),
    /// A number.
    Immediate(i64),
    /// An address: a register and an offset from it.
    Memory {
        /// The offset from the address in `base`.
        offset: i64,
        /// The register that holds the address.
        base: u8,
    },
}

impl Operand {
    /// Returns the register called `name`.
    ///
    /// ## Panics
    ///
    /// Panics if no register has that name.
    pub const fn register(name: &str) -> Self {
        match register_index(name) {
            Some(index) => Self::Register(index),
            None => panic!("this is not the name of a register"),
        }
    }

    /// Returns the address `offset` bytes past the one in the register called `base`.
    ///
    /// ## Panics
    ///
    /// Panics if no register has that name.
    pub const fn memory(offset: i64, base: &str) -> Self {
        match register_index(base) {
            Some(base) => Self::Memory { offset, base },
            None => panic!("this is not the name of a register"),
        }
    }
}

/// Encodes the instruction with this mnemonic and these operands.
///
/// The operands are the ones the [`Format`] of the instruction lists, in that order. An offset is
/// in bytes, and the immediate of `lui` and `auipc` is the 20 bits they place above the low 12.
/// An instruction of the A extension takes its ordering constraints as a suffix of the mnemonic:
/// `.aq`, `.rl` or `.aqrl`.
///
/// ## Panics
///
/// Panics if the mnemonic is not one of an instruction that the crate features include, if the
/// operands are not the ones the instruction takes, or if one of them does not fit its field.
pub const fn assemble(mnemonic: &str, operands: &[Operand]) -> Word {
    use Operand::{Immediate, Memory, Register};

    let (opcode, ordering) = match Opcode::from_mnemonic(mnemonic) {
        Some(opcode) => (opcode, 0),
        None => match split_ordering(mnemonic) {
            Some(split) => split,
            None => panic!("this is not the mnemonic of an instruction"),
        },
    };

    let (rd, rs1, rs2, imm) = match (opcode.format(), operands) {
        (Format::None, []) => (0, 0, 0, 0),
        (Format::R, [Register(rd), Register(rs1), Register(rs2)]) => (*rd, *rs1, *rs2, 0),
        (Format::Sources, [Register(rs1), Register(rs2)]) => (0, *rs1, *rs2, 0),
        (Format::I, [Register(rd), Register(rs1), Immediate(imm)]) => (*rd, *rs1, 0, *imm),
        (Format::Load, [Register(rd), Memory { offset, base }]) => (*rd, *base, 0, *offset),
        (Format::S, [Register(rs2), Memory { offset, base }]) => (0, *base, *rs2, *offset),
        (Format::B, [Register(rs1), Register(rs2), Immediate(offset)]) => (0, *rs1, *rs2, *offset),
        (Format::U, [Register(rd), Immediate(upper @ 0..=0xf_ffff)]) => {
            (*rd, 0, 0, (*upper << 12) as i32 as i64)
        }
        (Format::J, [Register(rd), Immediate(offset)]) => (*rd, 0, 0, *offset),
        (Format::Lr, [Register(rd), Memory { offset: 0, base }]) => (*rd, *base, 0, ordering),
        (Format::Amo, [Register(rd), Register(rs2), Memory { offset: 0, base }]) => {
            (*rd, *base, *rs2, ordering)
        }
        (Format::Csr, [Register(rd), Immediate(csr), Register(rs1)]) => (*rd, *rs1, 0, *csr),
        (Format::CsrI, [Register(rd), Immediate(csr), Immediate(uimm @ 0..=31)]) => {
            (*rd, *uimm as u8, 0, *csr)
        }
        _ => panic!("these are not the operands of the instruction"),
    };
    if imm < i32::MIN as i64 || imm > i32::MAX as i64 {
        panic!("the immediate does not fit the instruction");
    }

    match (Instruction { opcode, rd, rs1, rs2, imm: imm as i32, length: 4 }).encode() {
        Some(word) => word,
        None => panic!("an operand does not fit the instruction, or the instruction is disabled"),
    }
}

/// Splits the mnemonic of an instruction of the A extension that ends in ordering constraints
/// into the opcode and the constraints as [`Instruction::imm`] holds them.
const fn split_ordering(mnemonic: &str) -> Option<(Opcode, i64)> {
    const SUFFIXES: [(&str, i32); 3] = [
        (".aqrl", Instruction::AQ | Instruction::RL),
        (".aq", Instruction::AQ),
        (".rl", Instruction::RL),
    ];

    let mut i = 0;
    while i < SUFFIXES.len() {
        let (suffix, ordering) = SUFFIXES[i];
        if let Some((stem, end)) =
            mnemonic.as_bytes().split_at_checked(mnemonic.len() - suffix.len())
        {
            if crate::arch::bytes_eq(end, suffix.as_bytes()) {
                // The stem ends where a character does, because the suffix is ASCII.
                return match core::str::from_utf8(stem) {
                    Ok(stem) => match Opcode::from_mnemonic(stem) {
                        Some(opcode) if matches!(opcode.format(), Format::Lr | Format::Amo) => {
                            Some((opcode, ordering as i64))
                        }
                        _ => None,
                    },
                    Err(_) => None,
                };
            }
        }
        i += 1;
    }
    None
}

/// Returns the little-endian bytes of `words`: the machine code of a sequence of 32-bit
/// instructions.
///
/// ## Panics
///
/// Panics if `N` is not four times the number of words.
pub const fn to_bytes<const N: usize>(words: &[Word]) -> [u8; N] {
    assert!(N == 4 * words.len());
    let mut bytes = [0; N];
    let mut i = 0;
    while i < N {
        bytes[i] = (words[i / 4] >> (8 * (i % 4))) as u8;
        i += 1;
    }
    bytes
}

/// Encodes one instruction written in assembly language as a [`Word`].
///
/// The instruction is checked and encoded when the program is compiled, so the macro can
/// initialize constants, and a mnemonic or operand that is wrong fails the build.
///
/// Registers go by their `x` numbers or the names of the standard calling convention. An
/// immediate is a constant expression of type `i64`; one that consists of a single identifier
/// needs braces around it to tell it from a register. An address is written `offset(register)`
/// with a literal offset, or `(register)`. Offsets of branches and jumps are in bytes from the
/// instruction, since there are no labels. [`Format`] lists the operands of each
/// kind of instruction, and [`assemble`] the remaining rules.
///
/// Instructions are always encoded in 32 bits, never in the 16 bits of the C extension.
///
/// ```
/// use brisc_isa::{rv, Instruction, Opcode};
///
/// assert_eq!(rv!(addi a0, zero, 5), 0x0050_0513);
/// assert_eq!(rv!(sd ra, -8(sp)), 0xfe11_3c23);
/// assert_eq!(rv!(lr.w.aqrl a0, (a1)), 0x1605_a52f);
///
/// const MSTATUS: i64 = 0x300;
/// const READ_MSTATUS: u32 = rv!(csrrs t0, {MSTATUS}, zero);
/// assert_eq!(Instruction::decode(READ_MSTATUS).unwrap().opcode, Opcode::Csrrs);
/// ```
#[macro_export]
macro_rules! rv {
    ($($instruction:tt)+) => {
        const {
            let [word] = $crate::__rv_words!([] $($instruction)+ ;);
            word
        }
    };
}

/// Encodes a sequence of instructions written in assembly language, separated by semicolons, as
/// the bytes of their machine code.
///
/// Each instruction is written as for [rv!](crate::rv) and takes four bytes. The result is an
/// array, computed when the program is compiled.
///
/// ```
/// use brisc_isa::rv_code;
///
/// let code = rv_code! {
///     addi a0, zero, 5;
///     bne a0, a1, -4;
///     ecall;
/// };
/// assert_eq!(code, [0x13, 0x05, 0x50, 0x00, 0xe3, 0x1e, 0xb5, 0xfe, 0x73, 0x00, 0x00, 0x00]);
/// ```
#[macro_export]
macro_rules! rv_code {
    ($($instructions:tt)+) => {{
        const WORDS: &[$crate::Word] = &$crate::__rv_words!([] $($instructions)+ ;);
        const BYTES: [u8; 4 * WORDS.len()] = $crate::asm::to_bytes(WORDS);
        BYTES
    }};
}

/// Turns instructions that each end in a semicolon into an array of their encodings.
///
/// The macro takes one instruction off the front per step and appends its encoding to the list
/// in brackets. There is an arm for every sequence of operands that an instruction can have.
/// Arms that take a register in some place come before the ones that take an expression there,
/// because a register name is an expression too.
#[doc(hidden)]
#[macro_export]
macro_rules! __rv_words {
    ([$($word:expr,)*]) => {
        [$($word,)*]
    };
    ([$($word:expr,)*] ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)*] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!($m $(. $s)*,),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:ident, $c:ident ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::register(stringify!($b)),
            $crate::asm::Operand::register(stringify!($c)),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:ident, ($c:ident) ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::register(stringify!($b)),
            $crate::asm::Operand::memory(0, stringify!($c)),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:ident, $c:expr ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::register(stringify!($b)),
            $crate::asm::Operand::Immediate($c),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:ident ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::register(stringify!($b)),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, ($b:ident) ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::memory(0, stringify!($b)),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:literal ($c:ident) ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::memory($b, stringify!($c)),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:expr, $c:ident ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::Immediate($b),
            $crate::asm::Operand::register(stringify!($c)),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:expr, $c:expr ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::Immediate($b),
            $crate::asm::Operand::Immediate($c),
        ),] $($rest)*)
    };
    ([$($word:expr,)*] $m:ident $(. $s:ident)* $a:ident, $b:expr ; $($rest:tt)*) => {
        $crate::__rv_words!([$($word,)* $crate::__rv_word!(
            $m $(. $s)*,
            $crate::asm::Operand::register(stringify!($a)),
            $crate::asm::Operand::Immediate($b),
        ),] $($rest)*)
    };
}

/// Encodes the instruction with the mnemonic before the first comma and the
/// [`Operand`]s after it.
#[doc(hidden)]
#[macro_export]
macro_rules! __rv_word {
    ($m:ident $(. $s:ident)*, $($operand:expr,)*) => {
        $crate::asm::assemble(
            concat!(stringify!($m) $(, ".", stringify!($s))*),
            &[$($operand,)*],
        )
    };
}

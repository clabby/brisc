//! Types for the RISC-V architecture.

use cfg_if::cfg_if;

/// The mask for a [Byte].
pub const BYTE_MASK: XWord = 0xFF;

/// The mask for a [HalfWord].
pub const HALF_WORD_MASK: XWord = 0xFFFF;

/// The mask for a [Word].
pub const WORD_MASK: XWord = 0xFFFF_FFFF;

/// The mask for a [DoubleWord].
pub const DOUBLE_WORD_MASK: DoubleWord = 0xFFFF_FFFF_FFFF_FFFF;

/// The mask for a [QuadWord].
pub const QUAD_WORD_MASK: QuadWord = 0xFFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF_FFFF;

/// A byte.
pub type Byte = u8;

/// A 2-byte half word.
pub type HalfWord = u16;

/// A 4-byte word.
pub type Word = u32;

/// An 8-byte double word.
pub type DoubleWord = u64;

/// A 16-byte quad word.
pub type QuadWord = u128;

cfg_if! {
    if #[cfg(feature = "64-bit")] {
        /// X-LEN describes the width of the architecture's word.
        pub const X_LEN: usize = 64;

        /// A mask for the shift amount.
        pub const SHIFT_MASK: XWord = 0x3F;

        /// A type alias for a value that is the width of the architecture's word (64-bit).
        pub type XWord = DoubleWord;

        /// A type alias for a value that is double the width of the architecture's word (128-bit).
        pub type DoubleXWord = QuadWord;

        /// A type alias for a signed value that is the width of the architecture's word (64-bit).
        pub type SXWord = i64;

        /// A type alias for a signed value that is double the width of the architecture's word (128-bit).
        pub type SDoubleXWord = i128;

        /// An atomic type of size [X_LEN].
        pub type AtomicXWord = core::sync::atomic::AtomicU64;
    } else {
        /// X-LEN describes the width of the architecture's word.
        pub const X_LEN: usize = 32;

        /// A mask for the shift amount.
        pub const SHIFT_MASK: XWord = 0x1F;

        /// A type alias for a value that is the width of the architecture's word (32-bit).
        pub type XWord = Word;

        /// A type alias for a value that is double the width of the architecture's word (64-bit).
        pub type DoubleXWord = DoubleWord;

        /// A type alias for a signed value that is the width of the architecture's word (32-bit).
        pub type SXWord = i32;

        /// A type alias for a signed value that is double the width of the architecture's word (64-bit).
        pub type SDoubleXWord = i64;

        /// An atomic type of size [X_LEN].
        pub type AtomicXWord = core::sync::atomic::AtomicU32;
    }
}

/// hardwired zero
pub const REG_ZERO: usize = 0;

/// return address
pub const REG_RA: usize = 1;

/// stack pointer
pub const REG_SP: usize = 2;

/// global pointer
pub const REG_GP: usize = 3;

/// thread pointer
pub const REG_TP: usize = 4;

/// temporary register 0
pub const REG_T0: usize = 5;

/// temporary register 1
pub const REG_T1: usize = 6;

/// temporary register 2
pub const REG_T2: usize = 7;

/// saved register / frame pointer
pub const REG_S0_FP: usize = 8;

/// saved register 1
pub const REG_S1: usize = 9;

/// function argument 0 / return value 0
pub const REG_A0: usize = 10;

/// function argument 1 / return value 1
pub const REG_A1: usize = 11;

/// function argument 2
pub const REG_A2: usize = 12;

/// function argument 3
pub const REG_A3: usize = 13;

/// function argument 4
pub const REG_A4: usize = 14;

/// function argument 5
pub const REG_A5: usize = 15;

/// function argument 6
pub const REG_A6: usize = 16;

/// function argument 7
pub const REG_A7: usize = 17;

/// saved register 2
pub const REG_S2: usize = 18;

/// saved register 3
pub const REG_S3: usize = 19;

/// saved register 4
pub const REG_S4: usize = 20;

/// saved register 5
pub const REG_S5: usize = 21;

/// saved register 6
pub const REG_S6: usize = 22;

/// saved register 7
pub const REG_S7: usize = 23;

/// saved register 8
pub const REG_S8: usize = 24;

/// saved register 9
pub const REG_S9: usize = 25;

/// saved register 10
pub const REG_S10: usize = 26;

/// saved register 11
pub const REG_S11: usize = 27;

/// temporary register 3
pub const REG_T3: usize = 28;

/// temporary register 4
pub const REG_T4: usize = 29;

/// temporary register 5
pub const REG_T5: usize = 30;

/// temporary register 6
pub const REG_T6: usize = 31;

/// The names that the standard calling convention gives the integer registers `x0` to `x31`.
pub const REGISTER_NAMES: [&str; 32] = [
    "zero", "ra", "sp", "gp", "tp", "t0", "t1", "t2", "s0", "s1", "a0", "a1", "a2", "a3", "a4",
    "a5", "a6", "a7", "s2", "s3", "s4", "s5", "s6", "s7", "s8", "s9", "s10", "s11", "t3", "t4",
    "t5", "t6",
];

/// Returns the index of the integer register called `name`: `x0` to `x31`, one of the
/// [REGISTER_NAMES], or `fp`, the other name of `s0`.
pub const fn register_index(name: &str) -> Option<u8> {
    match name.as_bytes() {
        b"fp" => return Some(REG_S0_FP as u8),
        [b'x', digit @ b'0'..=b'9'] => return Some(*digit - b'0'),
        [b'x', tens @ b'1'..=b'3', digit @ b'0'..=b'9'] => {
            let index = (*tens - b'0') * 10 + (*digit - b'0');
            return if index < 32 { Some(index) } else { None };
        }
        _ => {}
    }

    let mut index = 0;
    while index < REGISTER_NAMES.len() {
        if bytes_eq(REGISTER_NAMES[index].as_bytes(), name.as_bytes()) {
            return Some(index as u8);
        }
        index += 1;
    }
    None
}

/// Compares two byte strings, which `==` cannot do in a `const fn`.
pub(crate) const fn bytes_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

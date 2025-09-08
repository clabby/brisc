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

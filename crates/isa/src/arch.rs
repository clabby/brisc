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
pub const REG_ZERO: XWord = 0;

/// return address
pub const REG_RA: XWord = 1;

/// stack pointer
pub const REG_SP: XWord = 2;

/// global pointer
pub const REG_GP: XWord = 3;

/// thread pointer
pub const REG_TP: XWord = 4;

/// temporary register 0
pub const REG_T0: XWord = 5;

/// temporary register 1
pub const REG_T1: XWord = 6;

/// temporary register 2
pub const REG_T2: XWord = 7;

/// saved register / frame pointer
pub const REG_S0_FP: XWord = 8;

/// saved register 1
pub const REG_S1: XWord = 9;

/// function argument 0 / return value 0
pub const REG_A0: XWord = 10;

/// function argument 1 / return value 1
pub const REG_A1: XWord = 11;

/// function argument 2
pub const REG_A2: XWord = 12;

/// function argument 3
pub const REG_A3: XWord = 13;

/// function argument 4
pub const REG_A4: XWord = 14;

/// function argument 5
pub const REG_A5: XWord = 15;

/// function argument 6
pub const REG_A6: XWord = 16;

/// function argument 7
pub const REG_A7: XWord = 17;

/// saved register 2
pub const REG_S2: XWord = 18;

/// saved register 3
pub const REG_S3: XWord = 19;

/// saved register 4
pub const REG_S4: XWord = 20;

/// saved register 5
pub const REG_S5: XWord = 21;

/// saved register 6
pub const REG_S6: XWord = 22;

/// saved register 7
pub const REG_S7: XWord = 23;

/// saved register 8
pub const REG_S8: XWord = 24;

/// saved register 9
pub const REG_S9: XWord = 25;

/// saved register 10
pub const REG_S10: XWord = 26;

/// saved register 11
pub const REG_S11: XWord = 27;

/// temporary register 3
pub const REG_T3: XWord = 28;

/// temporary register 4
pub const REG_T4: XWord = 29;

/// temporary register 5
pub const REG_T5: XWord = 30;

/// temporary register 6
pub const REG_T6: XWord = 31;

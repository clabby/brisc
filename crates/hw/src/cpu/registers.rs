//! The integer register file.

use brisc_isa::XWord;
use core::fmt;

/// The integer register file.
///
/// `x0` always reads as zero.
#[derive(Clone, PartialEq, Eq)]
pub struct Registers(pub(super) [XWord; Self::LEN]);

impl Registers {
    /// The number of slots in the file.
    ///
    /// Slots 0 to 31 hold `x0` to `x31`. The length is a power of two so that an index masked
    /// with `LEN - 1` is always in bounds without a check.
    pub(super) const LEN: usize = 64;

    /// The slot that receives writes to `x0`.
    ///
    /// Decoded instructions that name `x0` as their destination write here instead, which keeps
    /// slot 0 zero without a check on every write.
    pub(super) const SINK: u8 = 32;

    /// Creates a register file with every register zeroed.
    pub(super) const fn new() -> Self {
        Self([0; Self::LEN])
    }

    /// Returns the value of register `index`.
    ///
    /// ## Panics
    ///
    /// Panics if `index` is not below 32.
    pub fn get(&self, index: usize) -> XWord {
        self.0[..32][index]
    }

    /// Sets register `index` to `value`. Writes to `x0` are discarded.
    ///
    /// ## Panics
    ///
    /// Panics if `index` is not below 32.
    pub fn set(&mut self, index: usize, value: XWord) {
        if index != 0 {
            self.0[..32][index] = value;
        }
    }
}

impl fmt::Debug for Registers {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list().entries(&self.0[..32]).finish()
    }
}

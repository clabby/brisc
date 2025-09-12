//! Memory related errors

use crate::memory::Address;
use core::fmt::Debug;
use thiserror::Error;

/// An error type for memory operations.
#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum MemoryError {
    /// The page at the given index could not be found.
    #[error("Page not found at page index {0:08x}")]
    PageNotFound(Address),
    /// Unaligned memory access.
    #[error("Unaligned memory access at address {0:08x}")]
    UnalignedAccess(Address),
}

/// Type alias for a [Result] with [Result::Err] = [MemoryError].
pub type MemoryResult<T> = Result<T, MemoryError>;

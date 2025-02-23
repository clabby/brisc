//! Memory related errors

use crate::memory::Address;
use alloc::string::String;
use core::fmt::{Debug, Display};
use thiserror::Error;

/// An error type for memory operations.
#[derive(Error, Debug, Clone, Eq, PartialEq)]
pub enum MemoryError<T = String>
where
    T: Display + Debug + Clone + Eq + PartialEq,
{
    /// The page at the given index could not be found.
    #[error("Page not found at page index {0:08x}")]
    PageNotFound(Address),
    /// Unaligned memory access.
    #[error("Unaligned memory access at address {0:08x}")]
    UnalignedAccess(Address),
    /// Custom memory error.
    #[error("Memory error: {0}")]
    Custom(#[from] T),
}

/// Type alias for a [Result] with [Result::Err] = [MemoryError].
pub type MemoryResult<T, E = String> = Result<T, MemoryError<E>>;

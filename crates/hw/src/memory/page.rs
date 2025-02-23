//! The [Page] type as well as associated page parameterization constants.

use brisc_isa::XWord;

/// The size of a page address in bits.
pub const PAGE_ADDRESS_SIZE: usize = 12;

/// The size of a [Page] in bytes (4kb).
pub const PAGE_SIZE: usize = 1 << PAGE_ADDRESS_SIZE;

/// The mask to apply to an [Address] to obtain the page address.
///
/// [Address]: super::Address
pub const PAGE_ADDRESS_MASK: usize = PAGE_SIZE - 1;

/// An empty page of memory, zeroed out.
pub const EMPTY_PAGE: Page = [0u8; PAGE_SIZE];

/// An index of a [Page] within an [Address].
///
/// [Address]: super::Address
pub type PageIndex = XWord;

/// A page of memory, representing [PAGE_SIZE] bytes of data.
pub type Page = [u8; PAGE_SIZE];

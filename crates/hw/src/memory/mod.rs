//! Guest memory.

use brisc_isa::XWord;

mod sparse;
pub use sparse::SparseMemory;

/// A guest memory address.
pub type Address = XWord;

/// The index of a page: an [Address] with the offset within the page shifted out.
pub type PageIndex = XWord;

/// The number of [Address] bits that select a byte within a page.
pub const PAGE_ADDRESS_SIZE: usize = 12;

/// The size of a page in bytes (4 KiB).
pub const PAGE_SIZE: usize = 1 << PAGE_ADDRESS_SIZE;

/// The mask to apply to an [Address] to obtain the offset within its page.
pub const PAGE_ADDRESS_MASK: usize = PAGE_SIZE - 1;

/// The largest [PageIndex].
pub const MAX_PAGE_INDEX: PageIndex = Address::MAX >> PAGE_ADDRESS_SIZE;

/// Returns the index of the page that contains `address`.
#[inline(always)]
pub const fn page_index(address: Address) -> PageIndex {
    address >> PAGE_ADDRESS_SIZE
}

/// Returns the offset of `address` within its page.
#[inline(always)]
pub const fn page_offset(address: Address) -> usize {
    address as usize & PAGE_ADDRESS_MASK
}

/// Returns the address of the first byte of `page`.
#[inline(always)]
pub const fn page_base(page: PageIndex) -> Address {
    page << PAGE_ADDRESS_SIZE
}

/// The guest's address space.
///
/// Every address is readable and writable. Memory that was never written reads as zero, and
/// reading it does not map a page. Accesses may have any alignment and may cross page boundaries;
/// addresses wrap around at the end of the address space.
///
/// A memory also tells the processor when the guest overwrites code. The processor calls
/// [Memory::watch] for every page it has decoded instructions from, and learns about later
/// writes to those pages from [Memory::store] and [Memory::pop_dirty].
pub trait Memory {
    /// Reads `N` little-endian bytes starting at `address`.
    fn load<const N: usize>(&mut self, address: Address) -> [u8; N];

    /// Writes `N` bytes starting at `address`.
    ///
    /// Returns `true` if the write touched a watched page.
    fn store<const N: usize>(&mut self, address: Address, bytes: [u8; N]) -> bool;

    /// Fills `buf` with the bytes starting at `address`.
    fn read(&mut self, address: Address, buf: &mut [u8]);

    /// Writes `data` starting at `address`.
    fn write(&mut self, address: Address, data: &[u8]);

    /// Starts watching `page` for writes.
    ///
    /// The first write to a watched page ends the watch and queues the page for
    /// [Memory::pop_dirty].
    fn watch(&mut self, page: PageIndex);

    /// Removes and returns a page that was written while it was watched.
    fn pop_dirty(&mut self) -> Option<PageIndex>;

    /// Returns the number of pages that have been written to or watched.
    fn page_count(&self) -> usize;
}

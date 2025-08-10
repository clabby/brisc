//! Memory bus interface for the Brisc VM.

use crate::memory::{
    MemoryResult, Page, PageIndex, PAGE_ADDRESS_MASK, PAGE_ADDRESS_SIZE, PAGE_SIZE,
};
use alloc::{format, string::String, vec::Vec};
use brisc_isa::{Byte, DoubleWord, HalfWord, Word, XWord};

/// Length (in bytes) of a [HalfWord].
const HW_LEN: usize = HalfWord::BITS as usize >> 3;

/// Length (in bytes) of a [Word].
const W_LEN: usize = Word::BITS as usize >> 3;

/// Length (in bytes) of a [DoubleWord].
const DW_LEN: usize = DoubleWord::BITS as usize >> 3;

/// Type alias for a memory address.
pub type Address = XWord;

/// The [Memory] trait defines the interface for the memory bus.
pub trait Memory {
    /// Returns the number of pages allocated within the [Memory].
    fn page_count(&self) -> usize;

    /// Allocate a new page within the [Memory], and return an exclusive mutable reference to it if
    /// it was successfully allocated.
    fn alloc(&mut self, page_index: PageIndex) -> MemoryResult<&mut Page>;

    /// Looks up a page in the [Memory] by its index, and returns a reference to it.
    fn page(&self, page_index: PageIndex) -> Option<&Page>;

    /// Looks up a page in the [Memory] by its index, and returns a mutable reference to it.
    fn page_mut(&mut self, page_index: PageIndex) -> Option<&mut Page>;

    /// Get an 8-bit [Byte] from memory.
    fn get_byte(&self, address: Address) -> MemoryResult<Byte> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;

        // Attempt to lookup the page in memory.
        self.page(page_index).map_or(Ok(0), |page| Ok(page[page_address]))
    }

    /// Set an 8-bit [Byte] in memory.
    fn set_byte(&mut self, address: Address, value: Byte) -> MemoryResult<()> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;

        // Attempt to lookup the page in memory.
        let page =
            if let Some(page) = self.page_mut(page_index) { page } else { self.alloc(page_index)? };

        page[page_address] = value;

        Ok(())
    }

    /// Get a 16-bit [HalfWord] from memory. Unaligned access is supported.
    fn get_halfword(&self, address: Address) -> MemoryResult<HalfWord> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;

        // Create a temporary buffer to store the halfword.
        let mut dat = [0u8; HW_LEN];
        let mut count = 0;

        // Attempt to read the halfword from a single page.
        if let Some(page) = self.page(page_index) {
            let dat_len = HW_LEN.min(PAGE_SIZE - page_address);
            dat[..dat_len].copy_from_slice(&page[page_address..page_address + dat_len]);
            count += dat_len;
        }

        // If the halfword read will cross a page boundary, read the rest from the next page.
        if count < HW_LEN {
            if let Some(page) = self.page(page_index + 1) {
                let dat_len = HW_LEN - count;
                dat[count..].copy_from_slice(&page[..dat_len]);
            }
        }

        Ok(HalfWord::from_le_bytes(dat))
    }

    /// Set a 16-bit [HalfWord] in memory. Unaligned access is supported.
    fn set_halfword(&mut self, address: Address, value: HalfWord) -> MemoryResult<()> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;
        let dat = value.to_le_bytes();

        // Attempt to lookup the page in memory, and allocate it if it does not exist.
        let page_one =
            if let Some(page) = self.page_mut(page_index) { page } else { self.alloc(page_index)? };

        // Write as much of the halfword to the first page as possible.
        let dat_len = HW_LEN.min(PAGE_SIZE - page_address);
        page_one[page_address..page_address + dat_len].copy_from_slice(&dat[..dat_len]);

        // If the halfword write will cross a page boundary, write the rest to the next page.
        if dat_len < HW_LEN {
            // Attempt to lookup the page in memory, and allocate it if it does not exist.
            let page_two = if let Some(page) = self.page_mut(page_index + 1) {
                page
            } else {
                self.alloc(page_index + 1)?
            };
            page_two[..HW_LEN - dat_len].copy_from_slice(&dat[dat_len..]);
        }

        Ok(())
    }

    /// Get a 32-bit [Word] from memory. Unaligned access is supported.
    fn get_word(&self, address: Address) -> MemoryResult<Word> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;

        // Create a temporary buffer to store the word.
        let mut dat = [0u8; W_LEN];
        let mut count = 0;

        // Attempt to read the word from a single page.
        if let Some(page) = self.page(page_index) {
            let dat_len = W_LEN.min(PAGE_SIZE - page_address);
            dat[..dat_len].copy_from_slice(&page[page_address..page_address + dat_len]);
            count += dat_len;
        }

        // If the word read will cross a page boundary, read the rest from the next page.
        if count < W_LEN {
            if let Some(page) = self.page(page_index + 1) {
                let dat_len = W_LEN - count;
                dat[count..].copy_from_slice(&page[..dat_len]);
            }
        }

        Ok(Word::from_le_bytes(dat))
    }

    /// Set a 32-bit [Word] in memory. Natural alignment is enforced.
    fn set_word(&mut self, address: Address, value: Word) -> MemoryResult<()> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;
        let dat = value.to_le_bytes();

        // Attempt to lookup the page in memory, and allocate it if it does not exist.
        let page_one =
            if let Some(page) = self.page_mut(page_index) { page } else { self.alloc(page_index)? };

        // Write as much of the word to the first page as possible.
        let dat_len = W_LEN.min(PAGE_SIZE - page_address);
        page_one[page_address..page_address + dat_len].copy_from_slice(&dat[..dat_len]);

        // If the word write will cross a page boundary, write the rest to the next page.
        if dat_len < W_LEN {
            // Attempt to lookup the page in memory, and allocate it if it does not exist.
            let page_two = if let Some(page) = self.page_mut(page_index + 1) {
                page
            } else {
                self.alloc(page_index + 1)?
            };
            page_two[..W_LEN - dat_len].copy_from_slice(&dat[dat_len..]);
        }

        Ok(())
    }

    /// Get a 64-bit [DoubleWord] from memory at a given 8-byte aligned address.
    /// Natural alignment is enforced.
    fn get_doubleword(&self, address: Address) -> MemoryResult<DoubleWord> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;

        // Create a temporary buffer to store the doubleword.
        let mut dat = [0u8; DW_LEN];
        let mut count = 0;

        // Attempt to read the doubleword from a single page.
        if let Some(page) = self.page(page_index) {
            let dat_len = DW_LEN.min(PAGE_SIZE - page_address);
            dat[..dat_len].copy_from_slice(&page[page_address..page_address + dat_len]);
            count += dat_len;
        }

        // If the word read will cross a page boundary, read the rest from the next page.
        if count < DW_LEN {
            if let Some(page) = self.page(page_index + 1) {
                let dat_len = DW_LEN - count;
                dat[count..].copy_from_slice(&page[..dat_len]);
            }
        }

        Ok(DoubleWord::from_le_bytes(dat))
    }

    /// Set a 64-bit [DoubleWord] in memory at a given unaligned address.
    /// Natural alignment is enforced.
    fn set_doubleword(&mut self, address: Address, value: DoubleWord) -> MemoryResult<()> {
        // Compute the page index and the memory address within it.
        let page_index = address >> PAGE_ADDRESS_SIZE;
        let page_address = address as usize & PAGE_ADDRESS_MASK;
        let dat = value.to_le_bytes();

        // Attempt to lookup the page in memory, and allocate it if it does not exist.
        let page_one =
            if let Some(page) = self.page_mut(page_index) { page } else { self.alloc(page_index)? };

        // Write as much of the doubleword to the first page as possible.
        let dat_len = DW_LEN.min(PAGE_SIZE - page_address);
        page_one[page_address..page_address + dat_len].copy_from_slice(&dat[..dat_len]);

        // If the doubleword write will cross a page boundary, write the rest to the next page.
        if dat_len < DW_LEN {
            // Attempt to lookup the page in memory, and allocate it if it does not exist.
            let page_two = if let Some(page) = self.page_mut(page_index + 1) {
                page
            } else {
                self.alloc(page_index + 1)?
            };
            page_two[..DW_LEN - dat_len].copy_from_slice(&dat[dat_len..]);
        }

        Ok(())
    }

    /// Set a range of memory at a given [Address].
    ///
    /// ## Takes
    /// - `address`: The address to set the memory at.
    /// - `data`: The data to set.
    ///
    /// ## Returns
    /// - `Ok(())` if the memory was successfully set.
    /// - `Err(_)` if the memory could not be set.
    fn set_memory_range(&mut self, address: Address, data: &mut &[u8]) -> MemoryResult<()> {
        let mut address = address;
        while !data.is_empty() {
            let page_index = address >> PAGE_ADDRESS_SIZE as u64;
            let page_address = address as usize & PAGE_ADDRESS_MASK;

            let page = if let Some(page) = self.page_mut(page_index) {
                page
            } else {
                self.alloc(page_index)?
            };

            let write_len = data.len().min(PAGE_SIZE - page_address);
            let write_dat = &data[..write_len];
            page[page_address..page_address + write_len].copy_from_slice(write_dat);

            address += write_len as Address;
            *data = &data[write_len..];
        }

        Ok(())
    }

    /// Read a range of memory at a given [Address].
    ///
    /// ## Takes
    /// - `address`: The address to set the memory at.
    /// - `len`: The number of bytes to read.
    ///
    /// ## Returns
    /// - `Ok(())` if the memory was successfully read.
    /// - `Err(_)` if the memory could not be read.
    fn read_memory_range(&mut self, address: Address, len: XWord) -> MemoryResult<Vec<u8>> {
        let mut data = Vec::with_capacity(len as usize);

        let mut address = address;
        while data.len() < len as usize {
            let page_index = address >> PAGE_ADDRESS_SIZE as u64;
            let page_address = address as usize & PAGE_ADDRESS_MASK;

            let page = if let Some(page) = self.page(page_index) {
                page
            } else {
                return Err(super::MemoryError::PageNotFound(page_index));
            };

            let read_len = (len as usize - data.len()).min(PAGE_SIZE - page_address);
            data.extend_from_slice(&page[page_address..page_address + read_len]);

            address += read_len as Address;
        }

        Ok(data)
    }

    /// Returns a human-readable string describing the size of the [Memory].
    fn usage(&self) -> String {
        let total = (self.page_count() * PAGE_SIZE) as u64;
        const UNIT: u64 = 1024;
        if total < UNIT {
            return format!("{total} B");
        }
        let mut div = UNIT;
        let mut exp = 0;
        let mut n = total / UNIT;
        while n >= UNIT {
            div *= UNIT;
            exp += 1;
            n /= UNIT;
        }
        format!("{:.1} {}iB", (total as f64) / (div as f64), ['K', 'M', 'G', 'T', 'P', 'E'][exp])
    }
}

//! Memory: what a hart reads instructions and data from and writes data to.

use brisc_isa::{is_compressed, HalfWord, Word, XWord};

mod sparse;
pub use sparse::{Growth, SparseMemory};

/// An address in memory.
pub type Address = XWord;

/// The index of a page: an [`Address`] with the offset within the page shifted out.
pub type PageIndex = XWord;

/// The number of [`Address`] bits that select a byte within a page.
pub const PAGE_ADDRESS_SIZE: usize = 12;

/// The size of a page in bytes (4 KiB).
pub const PAGE_SIZE: usize = 1 << PAGE_ADDRESS_SIZE;

/// The mask to apply to an [`Address`] to obtain the offset within its page.
pub const PAGE_ADDRESS_MASK: usize = PAGE_SIZE - 1;

/// The largest [`PageIndex`].
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

/// The address space of a hart: memory, and whatever devices are mapped next to it.
///
/// An implementation needs [`Memory::load`] and [`Memory::store`]. The other methods have defaults
/// that are built on those two.
///
/// # Accesses
///
/// An access may have any alignment and may span pages, and addresses wrap around at the end
/// of the address space. An access that the memory cannot perform returns [`Memory::Error`]. The
/// hart then stops without retiring the instruction and without having changed a register. A store
/// that fails should not have written anything. A memory on which every access succeeds uses
/// [Infallible](core::convert::Infallible) as its error, and the hart's checks compile to
/// nothing.
///
/// # Code that changes
///
/// The hart decodes every instruction once and keeps the decoded form, so it has to hear about
/// writes to the bytes it decoded. It learns about them through three methods:
///
/// - The hart calls [`Memory::watch`] for every page that holds a byte of an instruction it keeps
///   in decoded form, before it executes the instruction.
/// - The first write to a watched page, by [`Memory::store`] or [`Memory::write`], ends the watch
///   and queues the page for [`Memory::pop_dirty`]. A store that does this returns `Ok(true)`.
/// - The hart empties the queue with [`Memory::pop_dirty`] when it starts to run and after every
///   store that returned `Ok(true)`, and decodes the instructions of those pages again when it next
///   executes them.
///
/// # Charging for accesses
///
/// A memory can ask for a price for an access, in the units of the budget of
/// [`Cpu::run`](crate::cpu::Cpu::run), on top of the weight of the instruction that makes it: for
/// example for growing. It sets [`Memory::CHARGES`] and answers [`Memory::access_cost`] before the
/// hart performs a load or a store. The hart pays the price or, if the budget does not cover the
/// instruction and its accesses, stops with [`Stop::Budget`](crate::cpu::Stop::Budget) before the
/// instruction has done anything. Whoever accesses the memory by other means, such as a kernel
/// that calls [`Memory::write`], pays for it itself.
///
/// The defaults of the methods that follow never report a write. They suit a memory on which the
/// instructions that the hart has executed do not change afterwards, such as one that keeps
/// its code in read-only memory. If they do change on such a memory, the hart goes on executing
/// the instructions it decoded first.
pub trait Memory {
    /// The reason an access failed.
    type Error;

    /// Whether [`Memory::access_cost`] can be more than zero. A memory that never charges leaves
    /// this unset, and the hart then does not ask.
    const CHARGES: bool = false;

    /// Returns what the load or the store of `len` bytes at `address` costs on top of the weight of
    /// its instruction, in the units of the budget. The hart asks before it performs the access,
    /// only if [`Memory::CHARGES`] is set, and does not perform it if the budget does not cover
    /// the price.
    ///
    /// The hart asks for every access that an instruction attempts, whatever comes of it: an
    /// atomic instruction asks as a store, also when a store-conditional loses its reservation and
    /// writes nothing. What a program costs therefore does not depend on whether the reservation
    /// held.
    ///
    /// The answer must be what the access costs now: repeating the question without changing the
    /// memory repeats the answer, and an access that costs nothing leaves the memory as the
    /// question found it. An access that a software TLB serves, which compiled code performs
    /// without asking, must cost nothing.
    fn access_cost(&self, address: Address, len: usize, write: bool) -> u64 {
        let _ = (address, len, write);
        0
    }

    /// Reads `N` bytes starting at `address`.
    fn load<const N: usize>(&mut self, address: Address) -> Result<[u8; N], Self::Error>;

    /// Writes `N` bytes starting at `address`.
    ///
    /// Returns `true` if the write ended the watch on a page.
    fn store<const N: usize>(
        &mut self,
        address: Address,
        bytes: [u8; N],
    ) -> Result<bool, Self::Error>;

    /// Reads the instruction that starts at `address` in order to execute it.
    ///
    /// The instruction is returned in the low bits, in little-endian byte order. If it is 16
    /// bits long, the upper half of the result has no meaning.
    ///
    /// The default loads the first 16 bits and, if they begin a 32-bit instruction, the 16
    /// bits after them, so it never touches a byte that is not part of the instruction.
    fn fetch(&mut self, address: Address) -> Result<Word, Self::Error> {
        let low = HalfWord::from_le_bytes(self.load(address)?) as Word;
        if is_compressed(low) {
            return Ok(low);
        }
        let high = HalfWord::from_le_bytes(self.load(address.wrapping_add(2))?) as Word;
        Ok(high << 16 | low)
    }

    /// Fills `buf` with the bytes starting at `address`.
    fn read(&mut self, address: Address, buf: &mut [u8]) -> Result<(), Self::Error> {
        for (i, byte) in buf.iter_mut().enumerate() {
            [*byte] = self.load(address.wrapping_add(i as Address))?;
        }
        Ok(())
    }

    /// Writes `data` starting at `address`.
    fn write(&mut self, address: Address, data: &[u8]) -> Result<(), Self::Error> {
        for (i, byte) in data.iter().enumerate() {
            self.store(address.wrapping_add(i as Address), [*byte])?;
        }
        Ok(())
    }

    /// Starts watching `page` for writes.
    fn watch(&mut self, page: PageIndex) {
        let _ = page;
    }

    /// Removes and returns a page that was written while it was watched.
    fn pop_dirty(&mut self) -> Option<PageIndex> {
        None
    }

    /// Describes a software TLB that compiled code may probe to access memory without calling
    /// [`Memory::load`] and [`Memory::store`], or returns `None` if there is none.
    ///
    /// Compiled code performs the accesses that hit the software TLB itself, so the implementation
    /// does not see them, whether they are loads or stores. An implementation therefore only
    /// describes a software TLB that serves accesses that it would perform in the same way, with no
    /// error, no side effect, no price and no need to hear about them: the contract of
    /// [`brisc_jit::Probe::new`], which is unsafe for that reason. An implementation that wraps
    /// another one must not forward its probe unless it adds none of those.
    ///
    /// The pointer must stay valid until the memory is used or moved.
    #[cfg(feature = "jit")]
    fn probe(&mut self) -> Option<*const brisc_jit::Probe> {
        None
    }
}

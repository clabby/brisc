//! A sparse, paged [Memory].

use super::{page_index, page_offset, Address, Memory, PageIndex, PAGE_SIZE};
use alloc::{boxed::Box, vec, vec::Vec};
use core::fmt;
use hashbrown::HashMap;

/// The number of entries in the translation cache.
const TLB_ENTRIES: usize = 1 << 10;

/// A tag that matches no page: it is larger than [MAX_PAGE_INDEX].
///
/// [MAX_PAGE_INDEX]: super::MAX_PAGE_INDEX
const NO_PAGE: PageIndex = PageIndex::MAX;

/// A cached translation from a page to the frame that backs it.
#[derive(Debug, Clone, Copy)]
struct TlbEntry {
    /// The page this entry translates for loads, or [NO_PAGE].
    read: PageIndex,
    /// The page this entry translates for stores, or [NO_PAGE].
    ///
    /// Set only when the page has a frame of its own and is not watched, so that the first store
    /// to an unmapped page and every store to a watched page take the slow path.
    write: PageIndex,
    /// The position of the frame within [SparseMemory::frames].
    frame: usize,
}

/// The backing store of one page.
///
/// Frames are aligned to their size, so an access that is aligned in the guest is aligned on the
/// host and never straddles a host cache line or page.
#[derive(Clone)]
#[repr(C, align(4096))]
struct Frame([u8; PAGE_SIZE]);

const _: () = assert!(align_of::<Frame>() == PAGE_SIZE && size_of::<Frame>() == PAGE_SIZE);

/// A [Memory] that maps pages on first write.
///
/// Pages are backed by page-aligned 4 KiB frames in one contiguous allocation. A direct-mapped
/// translation cache in front of the page table turns the common load or store into a tag
/// comparison and a copy.
#[derive(Clone)]
pub struct SparseMemory {
    /// The frames. Frame 0 is never written and backs every unmapped page.
    frames: Vec<Frame>,
    /// The frame number of every mapped page.
    pages: HashMap<PageIndex, u32>,
    /// Whether each frame's page is watched.
    watched: Vec<bool>,
    /// Pages written while watched, not yet returned from [Memory::pop_dirty].
    dirty: Vec<PageIndex>,
    /// The translation cache, indexed by the low bits of the page index.
    tlb: Box<[TlbEntry; TLB_ENTRIES]>,
}

impl Default for SparseMemory {
    fn default() -> Self {
        Self::new()
    }
}

impl SparseMemory {
    /// Creates an empty memory.
    pub fn new() -> Self {
        let tlb = vec![TlbEntry { read: NO_PAGE, write: NO_PAGE, frame: 0 }; TLB_ENTRIES];
        Self {
            frames: vec![Frame([0; PAGE_SIZE])],
            pages: HashMap::new(),
            watched: vec![false],
            dirty: Vec::new(),
            tlb: tlb.into_boxed_slice().try_into().expect("the TLB has TLB_ENTRIES entries"),
        }
    }

    /// Returns the frame number backing `page` and caches the translation.
    ///
    /// An unmapped page is given a frame of its own if `map` is set and is backed by the zero
    /// frame otherwise.
    #[cold]
    #[inline(never)]
    fn translate(&mut self, page: PageIndex, map: bool) -> usize {
        let frame = match self.pages.get(&page) {
            Some(frame) => *frame as usize,
            None if map => {
                let frame = self.frames.len();
                self.frames.push(Frame([0; PAGE_SIZE]));
                self.watched.push(false);
                self.pages.insert(page, frame as u32);
                frame
            }
            None => 0,
        };

        let writable = frame != 0 && !self.watched[frame];
        self.tlb[page as usize & (TLB_ENTRIES - 1)] =
            TlbEntry { read: page, write: if writable { page } else { NO_PAGE }, frame };
        frame
    }

    /// Calls `f` with each page-bounded piece of the `len` bytes starting at `address`: the
    /// range of the piece within the `len` bytes, and the bytes of its frame that it covers.
    fn for_each_piece(
        &mut self,
        mut address: Address,
        len: usize,
        map: bool,
        mut f: impl FnMut(core::ops::Range<usize>, &mut [u8]),
    ) {
        let mut done = 0;
        while done < len {
            let page = page_index(address);
            let offset = page_offset(address);
            let piece = (len - done).min(PAGE_SIZE - offset);

            let frame = self.translate(page, map);
            f(done..done + piece, &mut self.frames[frame].0[offset..offset + piece]);
            if map && self.watched[frame] {
                self.watched[frame] = false;
                self.dirty.push(page);
            }

            address = address.wrapping_add(piece as Address);
            done += piece;
        }
    }
}

impl fmt::Debug for SparseMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SparseMemory").field("pages", &self.pages.len()).finish_non_exhaustive()
    }
}

impl Memory for SparseMemory {
    #[inline(always)]
    fn load<const N: usize>(&mut self, address: Address) -> [u8; N] {
        let page = page_index(address);
        let offset = page_offset(address);
        let entry = self.tlb[page as usize & (TLB_ENTRIES - 1)];

        if entry.read == page && offset + N <= PAGE_SIZE {
            let mut bytes = [0; N];
            bytes.copy_from_slice(&self.frames[entry.frame].0[offset..offset + N]);
            bytes
        } else {
            let mut bytes = [0; N];
            self.read(address, &mut bytes);
            bytes
        }
    }

    #[inline(always)]
    fn store<const N: usize>(&mut self, address: Address, bytes: [u8; N]) -> bool {
        let page = page_index(address);
        let offset = page_offset(address);
        let entry = self.tlb[page as usize & (TLB_ENTRIES - 1)];

        if entry.write == page && offset + N <= PAGE_SIZE {
            self.frames[entry.frame].0[offset..offset + N].copy_from_slice(&bytes);
            false
        } else {
            let dirty = self.dirty.len();
            self.write(address, &bytes);
            self.dirty.len() != dirty
        }
    }

    #[cold]
    #[inline(never)]
    fn read(&mut self, address: Address, buf: &mut [u8]) {
        self.for_each_piece(address, buf.len(), false, |range, frame| {
            buf[range].copy_from_slice(frame);
        });
    }

    #[cold]
    #[inline(never)]
    fn write(&mut self, address: Address, data: &[u8]) {
        self.for_each_piece(address, data.len(), true, |range, frame| {
            frame.copy_from_slice(&data[range]);
        });
    }

    fn watch(&mut self, page: PageIndex) {
        let frame = self.translate(page, true);
        self.watched[frame] = true;
        // Cache the translation again, now without the write tag.
        self.translate(page, false);
    }

    fn pop_dirty(&mut self) -> Option<PageIndex> {
        self.dirty.pop()
    }

    fn page_count(&self) -> usize {
        self.pages.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const PAGE: Address = PAGE_SIZE as Address;

    #[test]
    fn unmapped_memory_reads_as_zero_without_mapping() {
        let mut memory = SparseMemory::new();
        assert_eq!(memory.load::<8>(0x1234), [0; 8]);
        let mut buf = [1; 3 * PAGE_SIZE];
        memory.read(PAGE - 5, &mut buf);
        assert!(buf.iter().all(|byte| *byte == 0));
        assert_eq!(memory.page_count(), 0);
    }

    #[test]
    fn store_after_load_of_unmapped_page_is_visible() {
        let mut memory = SparseMemory::new();
        assert_eq!(memory.load::<4>(0x2000), [0; 4]);
        assert!(!memory.store(0x2000, [1, 2, 3, 4]));
        assert_eq!(memory.load::<4>(0x2000), [1, 2, 3, 4]);
        // The zero frame is still zero for other unmapped pages.
        assert_eq!(memory.load::<4>(0x3000), [0; 4]);
        assert_eq!(memory.page_count(), 1);
    }

    #[test]
    fn accesses_cross_page_boundaries() {
        let mut memory = SparseMemory::new();
        let bytes = [1, 2, 3, 4, 5, 6, 7, 8];
        for offset in 1..8 {
            let address = 7 * PAGE - offset;
            assert!(!memory.store(address, bytes));
            assert_eq!(memory.load::<8>(address), bytes);
            assert_eq!(memory.load::<1>(7 * PAGE), [bytes[offset as usize]]);
        }
        assert_eq!(memory.page_count(), 2);
    }

    #[test]
    fn unmapped_page_below_a_mapped_page_reads_as_zero() {
        let mut memory = SparseMemory::new();
        memory.store(2 * PAGE, [0xff; 8]);
        assert_eq!(memory.load::<8>(PAGE), [0; 8]);
        assert_eq!(memory.load::<4>(2 * PAGE - 2), [0, 0, 0xff, 0xff]);
    }

    #[test]
    fn addresses_wrap_around() {
        let mut memory = SparseMemory::new();
        memory.store(Address::MAX - 1, [1, 2, 3, 4]);
        assert_eq!(memory.load::<2>(Address::MAX - 1), [1, 2]);
        assert_eq!(memory.load::<2>(0), [3, 4]);
    }

    #[test]
    fn bulk_write_and_read_round_trip() {
        let mut memory = SparseMemory::new();
        let data = (0..3 * PAGE_SIZE + 17).map(|i| i as u8).collect::<Vec<_>>();
        memory.write(PAGE - 9, &data);
        let mut buf = vec![0; data.len()];
        memory.read(PAGE - 9, &mut buf);
        assert_eq!(buf, data);
        assert_eq!(memory.page_count(), 5);
    }

    #[test]
    fn first_write_to_a_watched_page_is_reported_once() {
        let mut memory = SparseMemory::new();
        memory.store(5 * PAGE, [1; 4]);
        memory.watch(5);
        memory.watch(6);
        assert_eq!(memory.load::<4>(5 * PAGE), [1; 4]);
        assert_eq!(memory.pop_dirty(), None);

        // A store that straddles both watched pages reports both.
        assert!(memory.store(6 * PAGE - 2, [2; 4]));
        assert_eq!(memory.load::<4>(6 * PAGE - 2), [2; 4]);
        let mut dirty = [memory.pop_dirty(), memory.pop_dirty(), memory.pop_dirty()];
        dirty.sort();
        assert_eq!(dirty, [None, Some(5), Some(6)]);

        // The watch ended with the first write.
        assert!(!memory.store(5 * PAGE, [3; 4]));
        memory.write(6 * PAGE, &[3; 4]);
        assert_eq!(memory.pop_dirty(), None);
    }

    #[test]
    fn bulk_write_to_a_watched_page_is_reported() {
        let mut memory = SparseMemory::new();
        memory.watch(1);
        memory.write(PAGE + 100, &[7; 16]);
        assert_eq!(memory.pop_dirty(), Some(1));
    }
}

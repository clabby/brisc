//! The cache of decoded instructions.

use super::{
    lower::lower,
    op::{self, narrow, wide, Op},
};
use crate::memory::{Address, Memory, PageIndex, PAGE_ADDRESS_SIZE, PAGE_SIZE};
use alloc::{boxed::Box, vec, vec::Vec};
use brisc_isa::Word;
use core::fmt;
use hashbrown::HashMap;

/// The number of instruction slots in a page: an instruction can start at every second byte.
pub(super) const SLOTS: usize = PAGE_SIZE / 2;

/// The decoded form of one page of guest memory.
///
/// The [SLOTS] instruction slots are followed by two [Op::PAGE_END] slots, which is as far as
/// the last instruction of the page can advance.
pub(super) type CodePage = [Op; SLOTS + 2];

/// The instructions decoded so far, by page.
#[derive(Default)]
pub(super) struct CodeCache {
    pub(super) pages: Vec<Box<CodePage>>,
    /// The position in [Self::pages] of each page that has been decoded from.
    index: HashMap<PageIndex, usize>,
    /// The page most recently looked up and its position in [Self::pages].
    last: Option<(PageIndex, usize)>,
}

impl CodeCache {
    /// Returns the position of the decoded form of `page`, creating it if needed.
    pub(super) fn position(&mut self, page: PageIndex) -> usize {
        if let Some((last, position)) = self.last {
            if last == page {
                return position;
            }
        }

        let position = *self.index.entry(page).or_insert_with(|| {
            let mut code = vec![Op::DECODE; SLOTS + 2];
            code[SLOTS..].fill(Op::PAGE_END);
            self.pages
                .push(code.into_boxed_slice().try_into().expect("a page has SLOTS + 2 slots"));
            self.pages.len() - 1
        });
        self.last = Some((page, position));
        position
    }

    /// Decodes the instructions of `page` from `slot` up to the first one that is already decoded
    /// or that execution cannot fall through.
    #[cold]
    pub(super) fn decode<M: Memory>(&mut self, memory: &mut M, page: PageIndex, mut slot: usize) {
        let position = self.position(page);
        memory.watch(page);

        while slot < SLOTS && self.pages[position][slot].code == narrow::DECODE {
            let address = (page << PAGE_ADDRESS_SIZE).wrapping_add((slot as Address) << 1);
            let op = lower(Word::from_le_bytes(memory.load(address))).unwrap_or(Op::ILLEGAL);
            let is_wide = op.code & op::WIDE != 0;
            self.pages[position][slot] = op;

            if is_wide && slot == SLOTS - 1 {
                // The instruction continues on the next page.
                memory.watch(page.wrapping_add(1) & (PageIndex::MAX >> PAGE_ADDRESS_SIZE));
            }
            if matches!(
                op.code,
                narrow::ILLEGAL | narrow::JAL | wide::JAL | narrow::JALR | wide::JALR
            ) {
                break;
            }
            slot += if is_wide { 2 } else { 1 };
        }
    }

    /// Forgets the instructions decoded from the bytes of `page`.
    #[cold]
    pub(super) fn invalidate(&mut self, page: PageIndex) {
        if let Some(position) = self.index.get(&page) {
            self.pages[*position][..SLOTS].fill(Op::DECODE);
        }
        // The last instruction of the previous page may continue on this one.
        let previous = page.wrapping_sub(1) & (PageIndex::MAX >> PAGE_ADDRESS_SIZE);
        if let Some(position) = self.index.get(&previous) {
            self.pages[*position][SLOTS - 1] = Op::DECODE;
        }
    }
}

impl fmt::Debug for CodeCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodeCache").field("pages", &self.pages.len()).finish_non_exhaustive()
    }
}

//! The cache of decoded instructions.

use super::op::{code, decode, Op};
use crate::bus::{page_base, page_offset, Address, Bus, PageIndex, MAX_PAGE_INDEX, PAGE_SIZE};
use alloc::{boxed::Box, vec, vec::Vec};
use brisc_isa::{is_compressed, Opcode};
use core::fmt;
use hashbrown::HashMap;

/// The number of instruction slots in a page: an instruction can start at every second byte.
pub(super) const SLOTS: usize = PAGE_SIZE / 2;

/// Returns the position within its [CodePage] of the slot for the instruction at `address`.
#[inline(always)]
pub(super) const fn slot_of(address: Address) -> usize {
    page_offset(address) >> 1
}

/// The decoded form of one page of guest memory.
///
/// The [SLOTS] instruction slots are followed by two [Op::PAGE_END] slots, which is as far as
/// the last instruction of the page can advance.
pub(super) type CodePage = [Op; SLOTS + 2];

/// The instructions decoded so far, by page.
#[derive(Default)]
pub(super) struct CodeCache {
    pub(super) pages: Vec<Box<CodePage>>,
    /// How often control has flowed back to each slot of each page in [Self::pages].
    #[cfg(feature = "jit")]
    pub(super) counters: Vec<Box<[u16; SLOTS]>>,
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
            #[cfg(feature = "jit")]
            self.counters.push(Box::new([0; SLOTS]));
            self.pages.len() - 1
        });
        self.last = Some((page, position));
        position
    }

    /// Decodes the instructions of `page` from the slot `first` up to the first one that is
    /// already decoded or that execution cannot fall through.
    ///
    /// Returns the error of the bus if the instruction at `first` cannot be fetched. A later
    /// instruction that cannot be fetched is left for when execution reaches it.
    #[cold]
    pub(super) fn decode<B: Bus>(
        &mut self,
        bus: &mut B,
        page: PageIndex,
        first: usize,
    ) -> Result<(), B::Error> {
        let position = self.position(page);
        bus.watch(page);

        let mut slot = first;
        while slot < SLOTS && self.pages[position][slot].code() == code::DECODE {
            let address = page_base(page).wrapping_add((slot as Address) << 1);
            let word = match bus.fetch(address) {
                Ok(word) => word,
                Err(error) if slot == first => return Err(error),
                Err(_) => break,
            };
            if slot == SLOTS - 1 && !is_compressed(word) {
                // The word continues on the next page, whether or not it is an instruction.
                bus.watch(page.wrapping_add(1) & MAX_PAGE_INDEX);
            }

            let Ok(instruction) = decode(word) else {
                self.pages[position][slot] = Op::illegal(word);
                break;
            };
            self.pages[position][slot] = Op::new(instruction);
            if matches!(instruction.opcode, Opcode::Jal | Opcode::Jalr) {
                break;
            }
            slot += instruction.length as usize / 2;
        }
        Ok(())
    }

    /// Forgets the instructions decoded from the bytes of `page`.
    #[cold]
    pub(super) fn invalidate(&mut self, page: PageIndex) -> Option<usize> {
        let position = self.index.get(&page).copied();
        if let Some(position) = position {
            self.pages[position][..SLOTS].fill(Op::DECODE);
            #[cfg(feature = "jit")]
            self.counters[position].fill(0);
        }
        // The last instruction of the previous page may continue on this one.
        let previous = page.wrapping_sub(1) & MAX_PAGE_INDEX;
        if let Some(position) = self.index.get(&previous) {
            self.pages[*position][SLOTS - 1] = Op::DECODE;
        }
        position
    }
}

impl fmt::Debug for CodeCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CodeCache").field("pages", &self.pages.len()).finish_non_exhaustive()
    }
}

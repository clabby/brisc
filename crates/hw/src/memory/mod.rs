//! Simple memory implementation for the `brisc-hw` crate.

use hashbrown::HashMap;

mod interface;
pub use interface::{Address, Memory};

mod errors;
pub use errors::{MemoryError, MemoryResult};

mod page;
pub use page::{Page, PageIndex, EMPTY_PAGE, PAGE_ADDRESS_MASK, PAGE_ADDRESS_SIZE, PAGE_SIZE};

/// A simple memory implementation that uses a [`HashMap`] to store pages sparsely.
#[derive(Debug, Clone, Default)]
pub struct SimpleMemory(HashMap<PageIndex, Page>);

impl SimpleMemory {
    /// Create a new empty `SimpleMemory`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Return a reference to the underlying `HashMap`.
    const fn inner(&self) -> &HashMap<PageIndex, Page> {
        &self.0
    }

    /// Return a mutable reference to the underlying `HashMap`.
    const fn inner_mut(&mut self) -> &mut HashMap<PageIndex, Page> {
        &mut self.0
    }
}

impl Memory for SimpleMemory {
    fn page_count(&self) -> usize {
        self.inner().len()
    }

    fn alloc(&mut self, page_index: PageIndex) -> MemoryResult<&mut Page> {
        Ok(self.inner_mut().entry(page_index).or_insert_with(|| EMPTY_PAGE))
    }

    fn page(&self, page_index: PageIndex) -> Option<&Page> {
        self.inner().get(&page_index)
    }

    fn page_mut(&mut self, page_index: PageIndex) -> Option<&mut Page> {
        self.inner_mut().get_mut(&page_index)
    }
}

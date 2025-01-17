use crate::{
    memory_management::MemoryUsage,
    storage::{ComputeStorage, StorageHandle, StorageUtilization},
};

use alloc::vec::Vec;

use super::{calculate_padding, MemoryPool, Slice, SliceBinding, SliceHandle};

/// A memory pool that allocates buffers in a range of sizes and reuses them to minimize allocations.
///
/// - Only one slice is supported per page, due to the limitations in WGPU where each buffer should only bound with
///   either read only or read_write slices but not a mix of both.
/// - The pool uses a ring buffer to efficiently manage and reuse pages.
pub struct ExclusiveMemoryPool {
    pages: Vec<MemoryPage>,
    alignment: u64,
    dealloc_period: u64,
    last_dealloc_check: u64,

    min_alloc_size: u64,
    biggest_alloc: u64,
}

struct MemoryPage {
    slice: Slice,
    alloc_size: u64,
    free_count: u32,
}

// How much more to allocate (at most) than the requested allocation. This helps memory re-use, as a larger
// future allocation might be able to re-use the previous allocation.
const OVER_ALLOC: f64 = 1.35;
const ALLOC_AFTER_FREE: u32 = 5;

impl ExclusiveMemoryPool {
    pub(crate) fn new(min_alloc_size: u64, alignment: u64, dealloc_period: u64) -> Self {
        // Pages should be allocated to be aligned.
        assert_eq!(min_alloc_size % alignment, 0);
        Self {
            pages: Vec::new(),
            alignment,
            dealloc_period,
            last_dealloc_check: 0,
            min_alloc_size,
            biggest_alloc: min_alloc_size,
        }
    }

    /// Finds a free page that can contain the given size
    /// Returns a slice on that page if successful.
    fn get_free_page(&mut self, size: u64) -> Option<&mut MemoryPage> {
        // Return the smallest free page that fits.
        self.pages
            .iter_mut()
            .filter(|page| page.alloc_size >= size && page.slice.is_free())
            .min_by_key(|page| page.alloc_size)
    }

    fn alloc_page<Storage: ComputeStorage>(
        &mut self,
        storage: &mut Storage,
        size: u64,
    ) -> &mut MemoryPage {
        let alloc_size =
            ((size as f64 * OVER_ALLOC).round() as u64).next_multiple_of(self.alignment);
        let storage = storage.alloc(alloc_size);

        let handle = SliceHandle::new();
        let padding = calculate_padding(size, self.alignment);
        let mut slice = Slice::new(storage, handle, padding);

        // Return a smaller part of the slice. By construction, we only ever
        // get a page with a big enough size, so this is ok to do.
        slice.storage.utilization = StorageUtilization { offset: 0, size };
        slice.padding = padding;

        self.biggest_alloc = self.biggest_alloc.max(alloc_size);

        self.pages.push(MemoryPage {
            slice,
            alloc_size,
            free_count: 0,
        });

        let idx = self.pages.len() - 1;
        &mut self.pages[idx]
    }
}

impl MemoryPool for ExclusiveMemoryPool {
    /// Returns the resource from the storage, for the specified handle.
    fn get(&self, binding: &SliceBinding) -> Option<&StorageHandle> {
        let binding_id = *binding.id();
        self.pages
            .iter()
            .find(|page| page.slice.id() == binding_id)
            .map(|page| &page.slice.storage)
    }

    /// Reserves memory of specified size using the reserve algorithm, and return
    /// a handle to the reserved memory.
    ///
    /// Also clean ups, merging free slices together if permitted by the merging strategy
    fn try_reserve(&mut self, size: u64) -> Option<SliceHandle> {
        // Definitely don't have a slice this big.
        if size > self.biggest_alloc {
            return None;
        }

        let padding = calculate_padding(size, self.alignment);

        let page = self.get_free_page(size);

        if let Some(page) = page {
            // Return a smaller part of the slice. By construction, we only ever
            // get a page with a big enough size, so this is ok to do.
            page.slice.storage.utilization = StorageUtilization { offset: 0, size };
            page.slice.padding = padding;
            page.free_count = page.free_count.saturating_sub(1);

            return Some(page.slice.handle.clone());
        }

        None
    }

    fn alloc<Storage: ComputeStorage>(&mut self, storage: &mut Storage, size: u64) -> SliceHandle {
        assert!(
            size >= self.min_alloc_size,
            "Should allocate more than minimum size in pool!"
        );
        let page = self.alloc_page(storage, size);
        page.free_count = ALLOC_AFTER_FREE - 1;
        page.slice.handle.clone()
    }

    fn get_memory_usage(&self) -> MemoryUsage {
        let used_slices: Vec<_> = self
            .pages
            .iter()
            .filter(|page| !page.slice.is_free())
            .collect();

        MemoryUsage {
            number_allocs: used_slices.len() as u64,
            bytes_in_use: used_slices
                .iter()
                .map(|page| page.slice.storage.size())
                .sum(),
            bytes_padding: used_slices.iter().map(|page| page.slice.padding).sum(),
            bytes_reserved: self.pages.iter().map(|page| page.alloc_size).sum(),
        }
    }

    fn handles_alloc(&self, size: u64) -> bool {
        // Only handle slices in the range up to N times the min allocation size.
        size >= self.min_alloc_size
    }

    fn cleanup<Storage: ComputeStorage>(&mut self, storage: &mut Storage, alloc_nr: u64) {
        let check_period = self.dealloc_period / (ALLOC_AFTER_FREE as u64);

        if alloc_nr - self.last_dealloc_check < check_period {
            return;
        }

        self.last_dealloc_check = alloc_nr;

        self.pages.retain_mut(|page| {
            if page.slice.is_free() {
                page.free_count += 1;

                if page.free_count >= ALLOC_AFTER_FREE {
                    // Dealloc page.
                    storage.dealloc(page.slice.storage.id);
                    return false;
                }
            }

            true
        });
    }
}

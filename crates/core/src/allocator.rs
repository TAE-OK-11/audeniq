//! Global allocator of the long-running binaries (api, worker): jemalloc,
//! with large zeroed allocations zeroed in place.
//!
//! Argon2 asks for its 19 MiB of working memory with `alloc_zeroed`.
//! jemalloc serves a zeroed request on a reused extent by purging its pages
//! (`MALLOCX_ZERO`), so every password hash faulted ~4,900 fresh pages and
//! the login throughput dropped by a quarter (docs/BACKEND_RESOURCE_PERFORMANCE.md
//! "Allocator"). Above [`ZERO_IN_PLACE_MIN`] the memory is taken as is and
//! cleared with `memset`, as the system allocator does; the binaries also set
//! `oversize_threshold:0` so such blocks stay in the arenas and are reused.
use std::alloc::{GlobalAlloc, Layout};
use tikv_jemallocator::Jemalloc;

/// Smaller zeroed allocations keep jemalloc's own path (fresh pages are
/// already zero there, and small ones come from the thread cache).
pub const ZERO_IN_PLACE_MIN: usize = 1 << 20;

pub struct Allocator;

// SAFETY: every call is forwarded to jemalloc with the same layout; the
// zeroing path writes only the `layout.size()` bytes it was just given.
unsafe impl GlobalAlloc for Allocator {
    #[inline]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        unsafe { Jemalloc.alloc(layout) }
    }
    #[inline]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if layout.size() < ZERO_IN_PLACE_MIN {
            return unsafe { Jemalloc.alloc_zeroed(layout) };
        }
        let p = unsafe { Jemalloc.alloc(layout) };
        if !p.is_null() {
            unsafe { std::ptr::write_bytes(p, 0, layout.size()) };
        }
        p
    }
    #[inline]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { Jemalloc.dealloc(ptr, layout) }
    }
    #[inline]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        unsafe { Jemalloc.realloc(ptr, layout, new_size) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_zeroed_allocations_are_zero() {
        for size in [64, ZERO_IN_PLACE_MIN - 1, ZERO_IN_PLACE_MIN, 19 << 20] {
            let layout = Layout::from_size_align(size, 64).unwrap();
            unsafe {
                // Dirty a block of the same size first so the zeroed one may reuse it.
                let dirty = Allocator.alloc(layout);
                std::ptr::write_bytes(dirty, 0xa5, size);
                Allocator.dealloc(dirty, layout);
                let p = Allocator.alloc_zeroed(layout);
                assert!(!p.is_null());
                assert!(std::slice::from_raw_parts(p, size).iter().all(|b| *b == 0), "{size}");
                assert_eq!(p as usize % 64, 0);
                Allocator.dealloc(p, layout);
            }
        }
    }
}

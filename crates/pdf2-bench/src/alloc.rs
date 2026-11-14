//! A counting global allocator.
//!
//! Allocation **counts** are deterministic for the same input on every
//! platform, which makes them a reliable regression gate — unlike wall-clock
//! time, which is only comparable on the machine that produced the baseline.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicU64, Ordering};

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

/// The system allocator with two counters in front of it.
pub struct Counting;

// SAFETY: every method forwards to `System` with the same arguments it was
// given; the counters use relaxed atomics and do not affect allocation.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: `layout` is passed through unchanged.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(layout.size() as u64, Ordering::Relaxed);
        // SAFETY: `layout` is passed through unchanged.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller passes the same layout it allocated with.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES.fetch_add(new_size as u64, Ordering::Relaxed);
        // SAFETY: `ptr`, `layout` and `new_size` are passed through unchanged.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

/// Zero the counters, so the next measurement starts from a clean slate.
pub fn reset() {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

/// `(allocation count, allocated bytes)` since the last [`reset`].
pub fn snapshot() -> (u64, u64) {
    (
        ALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

#[cfg(test)]
mod tests {
    use super::{reset, snapshot};

    #[test]
    fn allocations_are_counted_between_snapshots() {
        reset();
        let first = snapshot();
        let buffer: Vec<u8> = vec![7; 4096];
        let second = snapshot();
        assert!(second.0 > first.0, "{first:?} -> {second:?}");
        assert!(second.1 >= first.1 + 4096, "{first:?} -> {second:?}");
        assert_eq!(buffer[0], 7);
        reset();
        assert_eq!(snapshot(), (0, 0));
    }
}

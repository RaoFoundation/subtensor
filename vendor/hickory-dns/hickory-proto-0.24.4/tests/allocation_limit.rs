//! Regression coverage for RUSTSEC-2026-0491.
//!
//! Runs in its own test process so allocator measurements cannot include
//! allocations made by parallel tests in the same process.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use hickory_proto_0244::op::{Header, Message};
use hickory_proto_0244::serialize::binary::{BinEncodable, BinEncoder};

struct CountingAllocator;

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::SeqCst) + layout.size();
            PEAK.fetch_max(live, Ordering::SeqCst);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::SeqCst);
        unsafe { System.dealloc(ptr, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

#[test]
fn forged_section_counts_have_bounded_initial_allocation() {
    let mut header = Header::new();
    header
        .set_query_count(u16::MAX)
        .set_answer_count(u16::MAX)
        .set_name_server_count(u16::MAX)
        .set_additional_count(u16::MAX);
    let mut encoded = Vec::new();
    header
        .emit(&mut BinEncoder::new(&mut encoded))
        .expect("the forged header encodes");

    PEAK.store(LIVE.load(Ordering::SeqCst), Ordering::SeqCst);
    assert!(Message::from_vec(&encoded).is_err());
    let allocated = PEAK.load(Ordering::SeqCst) - LIVE.load(Ordering::SeqCst);
    assert!(
        allocated < 64 * 1024,
        "malformed section counts allocated {allocated} bytes"
    );
}

//! Decode reads back what encode wrote, stays within each declared length, and allocates only
//! what the input can fill.

#![cfg(feature = "derive")]

use std::alloc::{GlobalAlloc, Layout as Alloc, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use layline::{Checksum, Layout, Message, ParseError};

/// The largest single allocation any test in this binary asked for.
struct Peak;

static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Peak {
    unsafe fn alloc(&self, layout: Alloc) -> *mut u8 {
        PEAK.fetch_max(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Alloc) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static GLOBAL: Peak = Peak;

struct Xor;

impl Checksum for Xor {
    type Output = u8;
    type State = u8;

    fn init() -> u8 {
        0
    }

    fn update(state: u8, bytes: &[u8]) -> u8 {
        bytes.iter().fold(state, |acc, &b| acc ^ b)
    }

    fn finish(state: u8) -> u8 {
        state
    }
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 2)]
struct Entry {
    value: u16,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Sought {
    off: u8,
    #[seek(off)]
    #[bytes(2)]
    entry: Entry,
    #[checksum(Xor, over = entry..)]
    lrc: u8,
}

#[test]
fn a_checksum_from_a_sought_record_covers_the_bytes_encode_covered() {
    let m = Sought { off: 0, entry: Entry { value: 0x1234 }, lrc: 0 };
    let wire = m.encode();
    assert_eq!(wire, [1, 0x34, 0x12, 0x34 ^ 0x12]);
    let (back, used) = Sought::decode(&wire).expect("reads back");
    assert_eq!((back.entry, back.lrc, used), (Entry { value: 0x1234 }, 0x26, 4));
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Block {
    n: u8,
    #[count(n)]
    body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Weightless {
    #[fill]
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Directory {
    a_off: u8,
    a_size: u8,
    b_off: u8,
    b_size: u8,
    #[seek(a_off)]
    #[when(a_size > 0)]
    #[message]
    a: Option<Block>,
    #[seek(b_off)]
    #[when(b_size > 0)]
    #[message]
    b: Option<Weightless>,
}

#[test]
fn a_measured_record_reads_only_its_measured_bytes() {
    let (m, _) = Directory::decode(&[6, 2, 4, 2, 7, 8, 1, 9]).expect("parses");
    assert_eq!(m.a, Some(Block { n: 1, body: vec![9] }));
    assert_eq!(m.b, Some(Weightless { data: vec![7, 8] }));
}

#[test]
fn a_measured_record_that_overruns_its_size_is_malformed() {
    assert!(matches!(
        Directory::decode(&[4, 1, 0, 0, 1, 9]),
        Err(ParseError::Malformed { field: "a_size", .. })
    ));
}

#[test]
fn a_measured_record_round_trips_with_the_size_encode_stamped() {
    let m = Directory {
        a_off: 0,
        a_size: 0,
        b_off: 0,
        b_size: 0,
        a: Some(Block { n: 2, body: vec![3, 4] }),
        b: Some(Weightless { data: vec![5] }),
    };
    let wire = m.encode();
    assert_eq!(wire, [4, 3, 7, 1, 2, 3, 4, 5]);
    assert_eq!(Directory::decode(&wire).expect("reads back").0.b, m.b);
}

#[derive(Debug, Clone, PartialEq, Layout)]
#[layout(bytes = 4096)]
struct Page {
    words: [u64; 512],
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Pages {
    n: u32,
    #[count(n)]
    #[bytes(4096)]
    pages: Vec<Page>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Strided {
    n: u32,
    stride: u16,
    #[count(n)]
    #[stride(stride)]
    #[bytes(4096)]
    pages: Vec<Page>,
}

#[test]
fn a_huge_count_allocates_by_the_bytes_the_wire_has() {
    assert!(matches!(Pages::decode(&[0xFF; 4]), Err(ParseError::Short { .. })));
    assert!(matches!(
        Strided::decode(&[0xFF, 0xFF, 0xFF, 0xFF, 0, 16]),
        Err(ParseError::Short { .. })
    ));
    let peak = PEAK.load(Ordering::Relaxed);
    assert!(peak < 1 << 20, "a 4-byte wire allocated {peak} bytes at once");
}

#[derive(Debug, Clone, PartialEq, Message)]
#[message(needs(stride: u8))]
struct Sized0 {
    #[len(stride)]
    data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Empties {
    k: u8,
    stride: u8,
    #[count(k)]
    #[with(stride)]
    #[message]
    items: Vec<Sized0>,
}

#[test]
fn a_counted_run_of_empty_messages_reads_back() {
    let m =
        Empties { k: 0, stride: 0, items: vec![Sized0 { data: vec![] }, Sized0 { data: vec![] }] };
    let wire = m.encode();
    assert_eq!(wire, [2, 0]);
    let (back, used) = Empties::decode(&wire).expect("reads back");
    assert_eq!((back.items, used), (m.items, 2));
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Window {
    lead: u16,
    len: u8,
    #[len(len)]
    items: Vec<u16>,
}

#[test]
fn a_window_its_elements_do_not_fill_is_malformed() {
    assert!(matches!(
        Window::decode(&[0, 0, 3, 1, 2, 3, 4, 5, 6]),
        Err(ParseError::Malformed { field: "items", at: 3 })
    ));
}

#[derive(Debug, Clone, PartialEq, Message)]
struct Far {
    lead: u16,
    off: u8,
    #[seek(off)]
    #[bytes(2)]
    entry: Entry,
}

#[test]
fn a_seek_past_the_end_reports_where_the_walk_stood() {
    assert_eq!(
        Far::decode(&[0, 0, 0xF0, 1, 2]),
        Err(ParseError::Short { need_bytes: 0xF0, got_bytes: 5, at: 3 })
    );
}

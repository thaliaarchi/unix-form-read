use std::{
    fs,
    mem::{self, offset_of},
};

use bstr::BStr;

/// A four-word header for a block (V5 form6.s).
#[derive(Clone, Debug)]
#[repr(C)]
struct Header {
    /// W - write ptr (also used as link ptr in frlist)
    write: u16,
    /// R - read ptr
    read: u16,
    /// A - pointer to head of data
    start: u16,
    /// L - ptr to (end+1) of data
    end: u16,
}

/// Header portion of `.bss` in the range of V5 form6.s:hblk..headend.
#[repr(C)]
struct Headers {
    /// Pointers to free block headers (V5 form6.s:frlist).
    free_list: [u16; 17],
    /// ? (V5 form6.s:asmdisc).
    asmdisc: u16,
    /// The block headers (V5 form6.s:headers).
    headers: [Header; HEADER_COUNT],
    pad: [u16; 2],
}

/// The size of the headers area (V5 form6.s:hsz).
const HEADERS_SIZE: u16 = 6144;
/// The size of the data area (V5 form6.s:datasz).
const DATA_SIZE: u16 = 32768;
const HEADER_COUNT: usize = (HEADERS_SIZE as usize - 36) / size_of::<Header>();

const _: () = assert!(size_of::<Headers>() == HEADERS_SIZE as usize);

fn main() {
    let form = fs::read("distr/form.m").unwrap();
    assert!(form.len() <= u16::MAX as usize);

    let headers: &[u8; HEADERS_SIZE as _] = form.first_chunk().unwrap();
    let headers: Headers = unsafe { mem::transmute(*headers) };

    let mut free_headers = [false; HEADER_COUNT];
    fn mark_free(free_headers: &mut [bool; HEADER_COUNT], headers: &Headers, header: u16) {
        if header == 0 {
            return;
        }
        let i = Header::index_from_pointer(header);
        let is_free = &mut free_headers[i];
        if *is_free {
            panic!("block header {header} referenced multiple times in free list");
        }
        *is_free = true;
        let next_free = headers.headers[i].write;
        mark_free(free_headers, headers, next_free);
    }
    for header in headers.free_list {
        mark_free(&mut free_headers, &headers, header);
    }

    println!("free_list: {:?}", headers.free_list);
    println!("asmdisc: {:?}", headers.asmdisc);
    for (i, header) in headers.headers.iter().enumerate() {
        let pointer = Header::pointer_from_index(i);
        let is_free = free_headers[i];
        if is_free {
            print!("{pointer}: free");
            if header.start != HEADERS_SIZE || header.end != HEADERS_SIZE {
                print!(" Header {{ start: {}, end: {} }}", header.start, header.end);
                // Observed invariant:
                assert!(header.read == 0 || header.read == HEADERS_SIZE);
            } else {
                // Observed invariant:
                assert!(header.read == HEADERS_SIZE);
            }
            println!();
            continue;
        }
        println!("{pointer}: alloc {header:?}");
        // Invariants from V5 form6.s:preposterous:
        assert!(
            header.start >= HEADERS_SIZE
                && header.end <= HEADERS_SIZE + DATA_SIZE
                && (header.end - header.start).is_power_of_two()
        );
        // Observed invariants:
        assert!(
            header.start <= header.end
                && (header.end as usize) <= form.len()
                && (header.start..=header.end).contains(&header.read)
                && (header.start..=header.end).contains(&header.write)
                && header.read <= header.write
        );
        println!(
            "  text: {:?}, slack: {:?}",
            BStr::new(&form[header.start as _..header.write as _]),
            BStr::new(&form[header.write as _..header.end as _])
        );
    }
    println!("pad: {:?}", headers.pad);

    let mut allocs = Vec::new();
    for (i, header) in headers.headers.iter().enumerate() {
        if !free_headers[i] {
            allocs.push(header.clone());
        }
    }
    allocs.sort_by(|x, y| x.start.cmp(&y.start).then(x.end.cmp(&y.end)));

    #[derive(Debug)]
    enum State {
        Alloc,
        Slack,
        Free,
    }
    let print_segment = |start: u16, end: u16, state: State| {
        println!(
            "offset={start}, len={len}, kind={state:?}, text={text:?}",
            len = end - start,
            text = BStr::new(&form[start as usize..end as usize])
        );
    };

    let mut prev_alloc = HEADERS_SIZE as u16;
    for header in &allocs {
        if header.start > prev_alloc {
            print_segment(prev_alloc, header.start, State::Free);
        }
        if header.start < prev_alloc {
            panic!("overlapping allocations");
        }

        print_segment(header.start, header.write, State::Alloc);
        if header.write != header.end {
            print_segment(header.write, header.end, State::Slack);
        }
        prev_alloc = header.end;
    }
    if (prev_alloc as usize) < form.len() {
        print_segment(prev_alloc, form.len() as u16, State::Free);
    }
}

impl Header {
    fn index_from_pointer(ptr: u16) -> usize {
        (ptr as usize - offset_of!(Headers, headers)) / size_of::<Header>()
    }

    fn pointer_from_index(index: usize) -> u16 {
        (offset_of!(Headers, headers) + size_of::<Header>() * index)
            .try_into()
            .unwrap()
    }
}

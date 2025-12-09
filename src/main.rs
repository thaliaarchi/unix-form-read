use std::{
    collections::HashSet,
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
const HEADERS_SIZE: usize = 6144;
/// The size of the data area (V5 form6.s:datasz).
const DATA_SIZE: usize = 32768;
const HEADER_COUNT: usize = (HEADERS_SIZE - 36) / size_of::<Header>();

const _: () = assert!(size_of::<Headers>() == HEADERS_SIZE);

fn main() {
    let form = fs::read("distr/form.m").unwrap();
    assert!(form.len() <= u16::MAX as usize);

    let strings = fs::read_to_string("strings.json").unwrap();
    let strings: Vec<String> = serde_json::from_str(&strings).unwrap();

    let headers: &[u8; HEADERS_SIZE] = form.first_chunk().unwrap();
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
        let status = if is_free { "free" } else { "alloc" };
        println!("{pointer}: {status} {header:?}");
        if is_free {
            continue;
        }
        // Invariants from V5 form6.s:preposterous:
        assert!(
            header.start as usize >= HEADERS_SIZE
                && header.end as usize <= HEADERS_SIZE + DATA_SIZE
                && (header.end - header.start).is_power_of_two()
        );
        // Assumed invariants:
        assert!(
            header.start <= header.end
                && (header.end as usize) <= form.len()
                && (header.start..=header.end).contains(&header.read)
                && (header.start..=header.end).contains(&header.write)
                && header.read <= header.write
        );
        let start = header.start as usize;
        println!("  end:   {:?}", BStr::new(&form[start..header.end as _]));
        println!("  read:  {:?}", BStr::new(&form[start..header.read as _]));
        println!("  write: {:?}", BStr::new(&form[start..header.write as _]));
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

    let mut alloc_strings = HashSet::new();

    let mut prev_alloc = HEADERS_SIZE as u16;
    for header in &allocs {
        if header.start > prev_alloc {
            print_segment(prev_alloc, header.start, State::Free);
        }
        if header.start < prev_alloc {
            panic!("overlapping allocations");
        }
        alloc_strings.insert(&form[header.start as usize..header.write as usize]);
        print_segment(header.start, header.write, State::Alloc);
        if header.write != header.end {
            print_segment(header.write, header.end, State::Slack);
        }
        prev_alloc = header.end;
    }
    if (prev_alloc as usize) < form.len() {
        print_segment(prev_alloc, form.len() as u16, State::Free);
    }

    for s in &strings {
        if !alloc_strings.contains(s.as_bytes()) {
            panic!("string not in allocated block: {s:?}");
        }
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

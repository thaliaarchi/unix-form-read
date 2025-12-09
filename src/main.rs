use std::{
    array, fs,
    mem::{self, offset_of},
};

use bstr::BStr;

struct Headers {
    headers: [Header; HEADER_COUNT],
    used: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum Header {
    Alloc { ptr: u16, len: u16, capacity: u16 },
    Freed { next: u16, ptr: u16, capacity: u16 },
    Unused { next: u16 },
}

/// A four-word header for a block (V5 form6.s).
#[derive(Clone, Debug)]
#[repr(C)]
struct RawHeader {
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
struct RawHeaders {
    /// Pointers to free block headers (V5 form6.s:frlist).
    free_list: [u16; 17],
    /// ? (V5 form6.s:asmdisc).
    asmdisc: u16,
    /// The block headers (V5 form6.s:headers).
    headers: [RawHeader; HEADER_COUNT],
    pad: [u16; 2],
}

/// The size of the headers area (V5 form6.s:hsz).
const HEADERS_SIZE: u16 = 6144;
/// The size of the data area (V5 form6.s:datasz).
const DATA_SIZE: u16 = 32768;
const HEADER_COUNT: usize = (HEADERS_SIZE as usize - 36) / size_of::<RawHeader>();

const _: () = assert!(size_of::<RawHeaders>() == HEADERS_SIZE as usize);

fn main() {
    let form = fs::read("distr/form.m").unwrap();
    assert!(form.len() <= u16::MAX as usize);

    let headers = Headers::from_form(&form);

    let mut allocs = Vec::new();
    for (i, header) in headers.headers[..headers.used].iter().enumerate() {
        let ptr = RawHeader::pointer_from_index(i);
        print!("{ptr}: {header:?}");
        if let &Header::Alloc { ptr, len, capacity } = header {
            let text = BStr::new(&form[ptr as usize..(ptr + len) as usize]);
            print!(": {text:?}");
            allocs.push((ptr, len, capacity));
        }
        println!();
    }
    allocs.sort();

    #[derive(Debug)]
    enum State {
        Alloc,
        Slack,
        Free,
    }
    let print_segment = |start: u16, end: u16, state: State| {
        println!(
            "offset={start}, len={len}, kind={state:?}, text={text:?}",
            text = BStr::new(&form[start as usize..end as usize]),
            len = end - start,
        );
    };

    let mut prev_alloc = HEADERS_SIZE as u16;
    for &(ptr, len, capacity) in &allocs {
        if ptr > prev_alloc {
            print_segment(prev_alloc, ptr, State::Free);
        }
        if ptr < prev_alloc {
            panic!("overlapping allocations");
        }
        let text_end = ptr + len;
        let alloc_end = ptr + capacity;
        print_segment(ptr, text_end, State::Alloc);
        if len != capacity {
            print_segment(text_end, alloc_end, State::Slack);
        }
        prev_alloc = alloc_end;
    }
    if prev_alloc < form.len() as u16 {
        print_segment(prev_alloc, form.len() as u16, State::Free);
    }
}

impl Headers {
    fn from_form(form: &[u8]) -> Self {
        let raw = RawHeaders::from_form(form);
        let mut free = [false; _];
        for &header in &raw.free_list {
            RawHeaders::visit_free(&mut free, &raw, header);
        }

        // Assumed invariant:
        assert_eq!(raw.asmdisc as usize, offset_of!(RawHeaders, headers));
        // Observed invariant:
        assert_eq!(raw.pad, [0, 0]);

        let form_len = form.len().try_into().unwrap();
        let parsed = array::from_fn(|i| {
            Header::from_raw(&raw.headers[i], free[i], form_len).expect("invalid header")
        });

        let mut used = parsed.len();
        if parsed[parsed.len() - 1] == (Header::Unused { next: 0 }) {
            used -= 1;
            let mut next = RawHeader::pointer_from_index(used);
            while used > 0 && parsed[used - 1] == { Header::Unused { next } } {
                used -= 1;
                next -= size_of::<Header>() as u16;
            }
        }
        for header in &parsed[..used] {
            if matches!(header, Header::Unused { .. }) {
                panic!("never-used header within allocated headers");
            }
        }

        Headers {
            headers: parsed,
            used,
        }
    }
}

impl RawHeaders {
    fn from_form(form: &[u8]) -> Self {
        let headers: &[u8; HEADERS_SIZE as _] = form.first_chunk().unwrap();
        unsafe { mem::transmute(*headers) }
    }

    fn visit_free(free: &mut [bool; HEADER_COUNT], headers: &RawHeaders, header: u16) {
        if header == 0 {
            return;
        }
        let i = RawHeader::index_from_pointer(header);
        let is_free = &mut free[i];
        if *is_free {
            panic!("block header {header} referenced multiple times in free list");
        }
        *is_free = true;
        let next_free = headers.headers[i].write;
        Self::visit_free(free, headers, next_free);
    }
}

impl Header {
    fn from_raw(header: &RawHeader, is_free: bool, form_len: u16) -> Option<Header> {
        if is_free {
            // Observed invariants:
            if header.start == HEADERS_SIZE && header.end == HEADERS_SIZE {
                if header.read != HEADERS_SIZE {
                    return None;
                }
                Some(Header::Unused { next: header.write })
            } else {
                if header.read != HEADERS_SIZE && header.read != 0 {
                    return None;
                }
                Some(Header::Freed {
                    next: header.write,
                    ptr: header.start,
                    capacity: header.end - header.start,
                })
            }
        } else {
            // Invariants from V5 form6.s:preposterous:
            if header.start >= HEADERS_SIZE
                && header.end <= HEADERS_SIZE + DATA_SIZE
                && (header.end - header.start).is_power_of_two()
                // Observed invariants:
                && header.start <= header.end
                && header.end <= form_len
                && (header.start..=header.end).contains(&header.read)
                && (header.start..=header.end).contains(&header.write)
                && header.read <= header.write
            {
                Some(Header::Alloc {
                    ptr: header.start,
                    len: header.write - header.start,
                    capacity: header.end - header.start,
                })
            } else {
                None
            }
        }
    }
}

impl RawHeader {
    fn index_from_pointer(ptr: u16) -> usize {
        (ptr as usize - offset_of!(RawHeaders, headers)) / size_of::<RawHeader>()
    }

    fn pointer_from_index(index: usize) -> u16 {
        (offset_of!(RawHeaders, headers) + size_of::<RawHeader>() * index)
            .try_into()
            .unwrap()
    }
}

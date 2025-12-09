use std::{
    collections::HashSet,
    fs,
    mem::{self, offset_of},
};

use bstr::BStr;

/// A four-word header for a block (V5 form6.s).
#[derive(Debug)]
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
        );
        let start = header.start as usize;
        println!("  end:   {:?}", BStr::new(&form[start..header.end as _]));
        println!("  read:  {:?}", BStr::new(&form[start..header.read as _]));
        println!("  write: {:?}", BStr::new(&form[start..header.write as _]));
    }
    println!("pad: {:?}", headers.pad);

    let mut labels = Vec::new();
    let mut offsets = HashSet::new();
    struct Label {
        offset: usize,
        len: usize,
        kind: Kind,
    }
    #[allow(dead_code)]
    #[derive(Debug)]
    enum Kind {
        String,
        Unknown,
        Header(Header),
    }
    let new_label = |offset, len, kind| Label { offset, len, kind };

    for s in &strings {
        let matches = form
            .windows(s.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == s.as_bytes()).then_some(offset));
        let mut found = false;
        for offset in matches {
            found = true;
            offsets.insert(offset);
            labels.push(new_label(offset, s.len(), Kind::String));
        }
        if !found {
            println!("// not found: {s:?}");
        }
    }

    for &offset in &offsets {
        let offset = u16::try_from(offset).unwrap();
        let offset_bytes = offset.to_le_bytes();
        let occurrences = form
            .windows(2)
            .enumerate()
            .filter_map(|(offset, window)| (window == offset_bytes).then_some(offset));
        for occurrence in occurrences {
            if occurrence < 4 || occurrence + 4 > form.len() {
                continue;
            }
            let i = occurrence - 4;
            let header: [u8; 8] = form[i..i + 8].try_into().unwrap();
            let header: Header = unsafe { mem::transmute(header) };
            if i % 8 == 4
                && header.start <= header.end
                && header.read <= header.end
                && header.write <= header.end
            {
                labels.push(new_label(i, 8, Kind::Header(header)));
            }
        }
    }

    labels.sort_by_key(|label| (label.offset, label.len));

    let mut segments = Vec::new();
    let mut push = |segment: Label| {
        println!(
            "offset={}, len={}, kind={:?}, text={:?}",
            segment.offset,
            segment.len,
            segment.kind,
            BStr::new(&form[segment.offset..segment.offset + segment.len])
        );
        segments.push(segment);
    };

    let mut offset = 0;
    for label in labels {
        if label.offset > offset {
            push(new_label(offset, label.offset - offset, Kind::Unknown));
        }
        if label.offset < offset {
            println!("// overlapping labels!");
        }
        offset = offset.max(label.offset + label.len);
        push(label);
    }
    if offset < form.len() {
        push(new_label(offset, form.len() - offset, Kind::Unknown));
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

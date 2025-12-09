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

/// Header data dumped from V5 form6.s:hblk..headend in `.bss`.
#[repr(C)]
struct Headers {
    /// Pointer to free header (V5 form6.s:hblk).
    free_header: u16,
    /// A list of pointers to free headers? (V5 form6.s:frlist).
    free_list: [u16; 16],
    /// ? (V5 form6.s:asmdisc).
    asmdisc: u16,
    /// The header blocks (V5 form6.s:headers).
    headers: [Header; 763],
    pad: [u16; 2],
}

/// The size of the headers area (V5 form6.s:hsz).
const HEADERS_SIZE: u16 = 6144;
/// The size of the data area (V5 form6.s:datasz).
const DATA_SIZE: u16 = 32768;

const _: () = assert!(size_of::<Headers>() == HEADERS_SIZE as usize);

fn main() {
    let form = fs::read("distr/form.m").unwrap();

    let strings = fs::read_to_string("strings.json").unwrap();
    let strings: Vec<String> = serde_json::from_str(&strings).unwrap();

    let headers: &[u8; HEADERS_SIZE as _] = form.first_chunk().unwrap();
    let headers: Headers = unsafe { mem::transmute(*headers) };
    println!("free_header: {:?}", headers.free_header);
    println!("free_list: {:?}", headers.free_list);
    println!("asmdisc: {:?}", headers.asmdisc);
    for (i, header) in headers.headers.iter().enumerate() {
        let offset = offset_of!(Headers, headers) + size_of::<Header>() * i;
        println!("{offset}: {header:?}");
        // The first two conditions are from V5 form6.s:preposterous;
        // the others are inferred.
        if header.start >= HEADERS_SIZE
            && header.end <= HEADERS_SIZE + DATA_SIZE
            && header.start <= header.end
            && (header.end as usize) <= form.len()
            && ((header.start..=header.end).contains(&header.read) || header.read == 0)
            && ((header.start..=header.end).contains(&header.write) || header.write == 0)
        {
            let start = header.start as usize;
            println!("  end:   {:?}", BStr::new(&form[start..header.end as _]));
            if header.read != 0 {
                println!("  read:  {:?}", BStr::new(&form[start..header.read as _]));
            } else {
                println!("  read:  None");
            }
            if header.write != 0 {
                println!("  write: {:?}", BStr::new(&form[start..header.write as _]));
            } else {
                println!("  write: None");
            }
        } else {
            println!("  Invalid!");
        }
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

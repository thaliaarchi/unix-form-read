use std::{collections::HashSet, fs, mem};

use bstr::BStr;

/// A four-word header for a block. Source: V5 form6.s.
#[derive(Debug)]
#[repr(C)]
struct Header {
    /// W - write ptr (also used as link ptr in frlist)
    write: u16,
    /// R - read ptr
    read: u16,
    /// A - pointer to head of data
    head: u16,
    /// L - ptr to (end+1) of data
    end: u16,
}

fn main() {
    let form = fs::read("distr/form.m").unwrap();

    let strings = fs::read_to_string("strings.json").unwrap();
    let strings: Vec<String> = serde_json::from_str(&strings).unwrap();

    let mut i = 4;
    while i + 8 < form.len() {
        let header: [u8; 8] = form[i..i + 8].try_into().unwrap();
        let header: Header = unsafe { mem::transmute(header) };
        println!("{i}: {header:?}");
        if header.head <= header.end
            && (header.end as usize) <= form.len()
            && ((header.head..=header.end).contains(&header.read) || header.read == 0)
            && ((header.head..=header.end).contains(&header.write) || header.write == 0)
        {
            let head = header.head as usize;
            println!("  end:   {:?}", BStr::new(&form[head..header.end as _]));
            if header.read != 0 {
                println!("  read:  {:?}", BStr::new(&form[head..header.read as _]));
            } else {
                println!("  read:  None");
            }
            if header.write != 0 {
                println!("  write: {:?}", BStr::new(&form[head..header.write as _]));
            } else {
                println!("  write: None");
            }
        } else {
            println!("  Invalid!");
        }
        i += 8;
    }

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
                && header.head <= header.end
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

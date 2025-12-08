use std::{
    fs,
    io::{Write, stdout},
};

use bstr::BStr;
use serde::Serialize;

fn main() {
    let form = fs::read("distr/form.m").unwrap();

    let strings = fs::read_to_string("strings.json").unwrap();
    let strings: Vec<String> = serde_json::from_str(&strings).unwrap();

    let mut labels = Vec::new();
    let mut offsets = Vec::new();
    #[derive(Serialize)]
    struct Label {
        offset: usize,
        len: usize,
        kind: Kind,
        text: String,
    }
    #[derive(Serialize)]
    enum Kind {
        String,
        Unknown,
        Offset(u16),
    }
    let new_label = |offset, len, kind| Label {
        offset,
        len,
        kind,
        text: BStr::new(&form[offset..offset + len]).to_string(),
    };

    for s in &strings {
        let matches = form
            .windows(s.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == s.as_bytes()).then_some(offset));
        let mut found = false;
        for offset in matches {
            found = true;
            offsets.push(offset);
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
            labels.push(new_label(occurrence, 2, Kind::Offset(offset)));
        }
    }

    labels.sort_by_key(|label| (label.offset, label.len));

    let mut out = stdout();
    let mut segments = Vec::new();
    let mut push = |segment| {
        serde_json::to_writer(&mut out, &segment).unwrap();
        out.write_all(b"\n").unwrap();
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

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Write, stdout},
};

use bstr::BStr;
use serde::Serialize;

fn main() {
    let form = fs::read("distr/form.m").unwrap();
    let text = fs::read_to_string("distr/y").unwrap();
    assert!(text.chars().all(|c| c.is_ascii()));

    #[derive(Default, Serialize)]
    struct Entry<'a> {
        text: &'a str,
        lines: Vec<Line<'a>>,
        offsets: Vec<Offset>,
    }
    #[derive(Serialize)]
    struct Line<'a> {
        name: &'a str,
        line: usize,
    }
    #[derive(Serialize)]
    struct Offset {
        offset: usize,
        occurrences: Vec<usize>,
    }

    let mut line_map = HashMap::<&str, Entry>::new();
    let mut names = HashSet::new();
    let mut name_number = None;

    for (line_number, mut line) in text.split("\n").enumerate() {
        if line.is_empty() {
            continue;
        }
        if let [b'n', b'a', b'm', b'e', b'-', .., b':'] = line.as_bytes() {
            let name = &line[5..line.len() - 1];
            assert!(names.insert(name), "non-unique name");
            name_number = Some(name);
            line = &line[..line.len() - 1];
        }
        let entry = line_map.entry(line).or_default();
        let name_number = name_number.expect("missing name number before first line");
        entry.text = line;
        entry.lines.push(Line {
            name: name_number,
            line: line_number,
        });
    }

    let mut labels = Vec::new();
    #[derive(Serialize)]
    struct Label {
        offset: usize,
        len: usize,
        label: String,
        text: String,
    }
    let new_label = |offset, len, label| Label {
        offset,
        len,
        label,
        text: BStr::new(&form[offset..offset + len]).to_string(),
    };

    for (&line, entry) in &mut line_map {
        let lines = entry
            .lines
            .iter()
            .map(|line| format!("{}:{}", line.name, line.line))
            .collect::<Vec<_>>()
            .join(",");
        let lines_str = format!("lines[{lines}]");
        let occurrences_str = format!("offset[{lines}]");

        let matches = form
            .windows(line.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == line.as_bytes()).then_some(offset));
        let offsets = matches.collect::<Vec<_>>();
        for &offset in &offsets {
            labels.push(new_label(offset, line.len(), lines_str.clone()));

            let offset_bytes = u16::try_from(offset).unwrap().to_le_bytes();
            let occurrences = form
                .windows(2)
                .enumerate()
                .filter_map(|(offset, window)| (window == offset_bytes).then_some(offset))
                .collect();
            for &occurrence in &occurrences {
                labels.push(new_label(
                    occurrence,
                    2,
                    format!("{occurrences_str} = {:?}", BStr::new(line)),
                ));
            }
            entry.offsets.push(Offset {
                offset,
                occurrences,
            });
        }
    }

    let mut entries = line_map.into_values().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.lines[0].line);
    let mut out = stdout();
    for entry in &entries {
        serde_json::to_writer(&mut out, entry).unwrap();
        out.write_all(b"\n").unwrap();
    }

    labels.sort_by(|x, y| {
        x.offset
            .cmp(&y.offset)
            .then(x.len.cmp(&y.len).reverse())
            .then_with(|| x.label.cmp(&y.label))
    });
    let mut segments = Vec::new();
    let mut offset = 0;
    let mut push = |label| {
        serde_json::to_writer(&mut out, &label).unwrap();
        out.write_all(b"\n").unwrap();
        segments.push(label);
    };
    for label in labels {
        if label.offset > offset {
            push(new_label(offset, label.offset - offset, "?".to_owned()));
        }
        if label.offset < offset {
            println!("// overlapping labels!");
        }
        offset = offset.max(label.offset + label.len);
        push(label);
    }
    if offset < form.len() {
        push(new_label(offset, form.len() - offset, "?".to_owned()));
    }
}

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Write, stdout},
};

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

    for (line_number, line) in text.split("\n").enumerate() {
        if line.is_empty() {
            continue;
        }
        if let [b'n', b'a', b'm', b'e', b'-', .., b':'] = line.as_bytes() {
            let name = &line[5..line.len() - 1];
            assert!(names.insert(name), "non-unique name");
            name_number = Some(name);
            continue;
        }
        let entry = line_map.entry(line).or_default();
        let name_number = name_number.expect("missing name number before first line");
        entry.text = line;
        entry.lines.push(Line {
            name: name_number,
            line: line_number,
        });
    }

    for (&line, entry) in &mut line_map {
        let matches = form
            .windows(line.len())
            .enumerate()
            .filter_map(|(offset, window)| (window == line.as_bytes()).then_some(offset));
        let offsets = matches.collect::<Vec<_>>();
        for &offset in &offsets {
            let offset_bytes = u16::try_from(offset).unwrap().to_le_bytes();
            let occurrences = form
                .windows(2)
                .enumerate()
                .filter_map(|(offset, window)| (window == offset_bytes).then_some(offset))
                .collect();
            entry.offsets.push(Offset {
                offset,
                occurrences,
            });
        }
    }

    let mut entries = line_map.into_values().collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.lines[0].line);
    let mut out = stdout().lock();
    for entry in &entries {
        serde_json::to_writer(&mut out, entry).unwrap();
        out.write_all(b"\n").unwrap();
    }
}

use std::{
    array,
    fmt::{self, Write},
    fs,
    mem::{self, offset_of},
};

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
    let form_len = u16::try_from(form.len()).unwrap();

    let headers = Headers::from_form(&form);

    println!("Headers:");
    for (i, header) in headers.headers[..headers.used].iter().enumerate() {
        let ptr = RawHeader::pointer_from_index(i);
        print!("{ptr}: {header:?}");
        if let &Header::Alloc { ptr, len, .. } = header {
            let text = Bytes(&form[ptr as usize..(ptr + len) as usize]);
            print!(": {text:?}");
        }
        println!();
    }
    println!();

    let mut allocs = Vec::new();
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
    enum State {
        Alloc,
        Slack,
        KnownFreed,
        Unknown,
    }

    for header in &headers.headers[..headers.used] {
        match *header {
            Header::Alloc { ptr, len, capacity } => {
                allocs.push((ptr, ptr + len, State::Alloc));
                if len != capacity {
                    allocs.push((ptr + len, ptr + capacity, State::Slack));
                }
            }
            Header::Freed { ptr, capacity, .. } => {
                allocs.push((ptr, ptr + capacity, State::KnownFreed));
            }
            Header::Unused { .. } => unreachable!(),
        }
    }
    allocs.sort();

    let mut prev_alloc = HEADERS_SIZE as u16;
    for i in 0..allocs.len() {
        let (start, end, _) = allocs[i];
        if start > prev_alloc {
            allocs.push((prev_alloc, start, State::Unknown));
        }
        if start < prev_alloc {
            panic!("overlapping allocations");
        }
        prev_alloc = end;
    }
    if prev_alloc < form_len {
        allocs.push((prev_alloc, form_len, State::Unknown));
    }
    allocs.sort();

    let mut freed_text = String::new();
    let mut freed_cells = vec![None; form.len()];

    println!("Allocations:");
    for &(start, end, state) in &allocs {
        let (i, j, truncated) = if state == State::KnownFreed && end > form_len {
            (start.min(form_len), end.min(form_len), "...")
        } else {
            (start, end, "")
        };
        let text = Bytes(&form[i as usize..j as usize]);
        let len = end - start;
        println!("offset={start}, len={len}, kind={state:?}, text={text:?}{truncated}");

        write!(freed_text, "«{start}:").unwrap();
        let short_state = match state {
            State::Alloc => 'a',
            State::Slack => 's',
            State::KnownFreed => 'f',
            State::Unknown => 'u',
        };
        write!(freed_text, "{short_state}»").unwrap();
        if state == State::Alloc {
            for _ in 0..len {
                freed_text.push('\u{FFFD}');
            }
        } else {
            write!(freed_text, "{text}").unwrap();
        }

        if state != State::Alloc {
            for i in i..j {
                freed_cells[i as usize] = Some(form[i as usize]);
            }
        }
    }
    println!();

    println!("Freed text:");
    println!("{freed_text}");

    let residual_strings: Vec<(usize, String)> =
        serde_json::from_str(&fs::read_to_string("residual.json").unwrap()).unwrap();
    for (start, string) in residual_strings {
        for (i, &b) in string.as_bytes().iter().enumerate() {
            if let Some(b2) = freed_cells[start + i]
                && b2 != b
            {
                let freed_cells_str = freed_cells[start..start + string.len()]
                    .iter()
                    .map(|c| c.unwrap_or(b'?'))
                    .collect::<Vec<u8>>();
                panic!(
                    "freed string does not match at byte {i}:\n  start = {start}\n  json =  {:?}\n  form =  {:?}\n  cells = {:?}\n",
                    Bytes(string.as_bytes()),
                    Bytes(&form[start..start + string.len()]),
                    Bytes(&freed_cells_str),
                );
            }
        }
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
                // Observed invariants:
                if header.start >= HEADERS_SIZE
                    && header.end <= HEADERS_SIZE + DATA_SIZE
                    && (header.end - header.start).is_power_of_two()
                    && header.start <= header.end
                    && (header.read == HEADERS_SIZE || header.read == 0)
                {
                    Some(Header::Freed {
                        next: header.write,
                        ptr: header.start,
                        capacity: header.end - header.start,
                    })
                } else {
                    None
                }
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

struct Bytes<'a>(&'a [u8]);

impl fmt::Debug for Bytes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{self}\"")
    }
}

impl fmt::Display for Bytes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for &b in self.0 {
            match b {
                b'\\' => f.write_str("\\\\"),
                b' '..=b'~' => f.write_char(b as char),
                b'\0' => f.write_str("\\0"),
                // 0x08 => f.write_str("\\b"),
                b'\t' => f.write_str("\\t"),
                b'\n' => f.write_str("\\n"),
                // 0x0B => f.write_str("\\v"),
                // 0x0C => f.write_str("\\f"),
                // 0x0D => f.write_str("\\r"),
                b => write!(f, "\\x{b:02x}"),
            }?;
        }
        Ok(())
    }
}

use std::{
    fmt::{self, Write},
    fs,
    mem::{self, offset_of},
    process::exit,
};

struct Headers {
    /// Index of the key-value table block in `self.headers`.
    table_index: usize,
    headers: Vec<Header>,
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
    /// Pointer to the key-value table, evidently "associative memory disc
    /// pointer" (V5 form6.s:asmdisc).
    table_ptr: u16,
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Error {
    FileTooLong { len: usize },
    FileTooShort { len: usize },
    HeaderPadNonZero { pad: [u16; 2] },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Error::FileTooLong { len } => write!(
                f,
                "form file is too long to be addressed by u16: length {len}",
            ),
            Error::FileTooShort { len } => write!(
                f,
                "form file is too short: length {len} cannot fit headers (length {HEADERS_SIZE})",
            ),
            Error::HeaderPadNonZero { pad } => write!(f, "header padding is non-zero: {pad:04x?}"),
        }
    }
}

fn main() {
    let form = fs::read("distr/form.m").unwrap();
    let form_len = u16::try_from(form.len()).unwrap();

    let headers = match Headers::from_form(&form) {
        Ok(headers) => headers,
        Err(err) => {
            eprintln!("{err}");
            exit(1);
        }
    };

    println!("Entries:");
    let table = match headers.headers[headers.table_index] {
        Header::Alloc { ptr, len, .. } => &form[ptr as usize..(ptr + len) as usize],
        _ => panic!("unallocated table"),
    };
    let (table_words, []) = table.as_chunks::<4>() else {
        panic!("table not divisible by 4");
    };
    for &entry in table_words {
        let key = u16::from_le_bytes(entry[..2].try_into().unwrap());
        let value = u16::from_le_bytes(entry[2..].try_into().unwrap());
        let Some(key) = RawHeader::index_from_pointer(key) else {
            panic!("key not a header pointer: {key}")
        };
        let Some(value) = RawHeader::index_from_pointer(value) else {
            panic!("value not a header pointer: {value}")
        };
        let Header::Alloc { ptr, len, .. } = headers.headers[key] else {
            panic!("unallocated key");
        };
        let key = Bytes(&form[ptr as usize..(ptr + len) as usize]);
        let Header::Alloc { ptr, len, .. } = headers.headers[value] else {
            panic!("unallocated value");
        };
        let value = Bytes(&form[ptr as usize..(ptr + len) as usize]);
        println!("{key:?}: {value:?}");
    }
    println!();

    println!("Headers:");
    for (i, header) in headers.headers.iter().enumerate() {
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

    for header in &headers.headers {
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
    fn from_form(form: &[u8]) -> Result<Self, Error> {
        let Ok(form_len) = form.len().try_into() else {
            return Err(Error::FileTooLong { len: form.len() });
        };
        let Some(raw) = form.first_chunk() else {
            return Err(Error::FileTooShort { len: form.len() });
        };

        let raw = RawHeaders::from_bytes(raw);
        let mut free = [false; _];
        for &header in &raw.free_list {
            RawHeaders::visit_free(&mut free, &raw, header);
        }

        let table_index =
            RawHeader::index_from_pointer(raw.table_ptr).expect("invalid table pointer");
        // Observed invariant:
        if raw.pad != [0, 0] {
            return Err(Error::HeaderPadNonZero { pad: raw.pad });
        }

        let mut parsed = Vec::with_capacity(HEADER_COUNT);
        for (i, header) in raw.headers.iter().enumerate() {
            parsed.push(header.parse(free[i], form_len).expect("invalid header"));
        }

        let mut next = 0;
        while parsed.last() == Some(&Header::Unused { next }) {
            parsed.pop();
            next = RawHeader::pointer_from_index(parsed.len());
        }

        for header in &parsed {
            if matches!(header, Header::Unused { .. }) {
                panic!("never-used header within allocated headers");
            }
        }

        Ok(Headers {
            table_index,
            headers: parsed,
        })
    }
}

impl RawHeaders {
    fn from_bytes(bytes: &[u8; HEADERS_SIZE as _]) -> Self {
        unsafe { mem::transmute(*bytes) }
    }

    fn visit_free(free: &mut [bool; HEADER_COUNT], headers: &RawHeaders, header: u16) {
        if header == 0 {
            return;
        }
        let Some(i) = RawHeader::index_from_pointer(header) else {
            panic!("invalid header pointer: {header}");
        };
        let is_free = &mut free[i];
        if *is_free {
            panic!("block header {header} referenced multiple times in free list");
        }
        *is_free = true;
        let next_free = headers.headers[i].write;
        Self::visit_free(free, headers, next_free);
    }
}

impl RawHeader {
    fn parse(&self, is_free: bool, form_len: u16) -> Option<Header> {
        if is_free {
            // Observed invariants:
            if self.start == HEADERS_SIZE && self.end == HEADERS_SIZE {
                if self.read != HEADERS_SIZE {
                    return None;
                }
                Some(Header::Unused { next: self.write })
            // Observed invariants:
            } else if self.start >= HEADERS_SIZE
                && self.end <= HEADERS_SIZE + DATA_SIZE
                && (self.end - self.start).is_power_of_two()
                && self.start <= self.end
                && (self.read == HEADERS_SIZE || self.read == 0)
            {
                Some(Header::Freed {
                    next: self.write,
                    ptr: self.start,
                    capacity: self.end - self.start,
                })
            } else {
                None
            }
        } else {
            // Invariants from V5 form6.s:preposterous:
            if self.start >= HEADERS_SIZE
                && self.end <= HEADERS_SIZE + DATA_SIZE
                && (self.end - self.start).is_power_of_two()
                // Observed invariants:
                && self.start <= self.end
                && self.end <= form_len
                && (self.start..=self.end).contains(&self.read)
                && (self.start..=self.end).contains(&self.write)
                && self.read <= self.write
            {
                Some(Header::Alloc {
                    ptr: self.start,
                    len: self.write - self.start,
                    capacity: self.end - self.start,
                })
            } else {
                None
            }
        }
    }

    fn index_from_pointer(ptr: u16) -> Option<usize> {
        let n = (ptr as usize).checked_sub(offset_of!(RawHeaders, headers))?;
        if n % size_of::<RawHeader>() != 0 {
            return None;
        }
        Some(n / size_of::<RawHeader>())
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

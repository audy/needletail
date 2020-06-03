#![no_main]
#[macro_use] extern crate libfuzzer_sys;
extern crate needletail;

use needletail::parser::{FastqReader, FastxReader};
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let cursor = Cursor::new([b"@", data].concat());
    let mut reader = FastqReader::new(cursor);
    while let Some(rec) = reader.next() {
        let _ = rec;
    }
});

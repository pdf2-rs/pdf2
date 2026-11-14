//! Direct objects and indirect objects, including stream payloads.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = pdf2::parse_object(data);
    let _ = pdf2::parse_indirect_object(data);
});

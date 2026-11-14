//! The content-stream interpreter: a fixed document supplies the fonts and
//! resources, the fuzz input is the content stream itself.
#![no_main]

use libfuzzer_sys::fuzz_target;

const FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/text.pdf");

fuzz_target!(|data: &[u8]| {
    let Ok(document) = pdf2::Document::open(FIXTURE) else {
        return;
    };
    let Ok(page) = document.page(0) else {
        return;
    };
    let _ = document.extract_text_from(data, page.resources.as_ref());
});

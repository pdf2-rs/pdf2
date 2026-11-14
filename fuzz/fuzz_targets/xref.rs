//! Opening a document: header, startxref, cross-reference tables and streams,
//! object fetching, page tree, content decoding and text extraction.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(document) = pdf2::Document::open(data) else {
        return;
    };
    let _ = document.version();
    let _ = document.metadata();
    if let Ok(pages) = document.pages() {
        for page in pages.iter().take(32) {
            let _ = document.page_content(page);
            let _ = document.extract_text(page);
        }
    }
    for number in 0..64 {
        let _ = document.get_object_by_number(number);
    }
});

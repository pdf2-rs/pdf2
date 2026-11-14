//! The security handler and decryption path: the fuzz input is a whole file,
//! opened with the right password and with none.
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for password in [&b"user"[..], b"", b"owner"] {
        let Ok(document) = pdf2::Document::open_with_password(data, password) else {
            continue;
        };
        let _ = document.is_encrypted();
        if let Ok(page) = document.page(0) {
            let _ = document.page_content(&page);
            let _ = document.extract_text(&page);
        }
    }
});

//! A tiny C ABI so a JavaScript host can drive this library in a
//! browser-target (`wasm32-unknown-unknown`) module without `wasm-bindgen`.
//!
//! Protocol: the host calls [`pdf2_input`] with the file length, writes the PDF
//! bytes to the returned pointer, then calls [`pdf2_pages`] and [`pdf2_text`],
//! and reads the extracted text back through [`pdf2_output_ptr`] and the length
//! [`pdf2_text`] returned.
//!
//! The `unsafe` blocks only ever dereference pointers into vectors this module
//! owns, and those vectors stay alive for the lifetime of the module, which is
//! why the crate allows `unsafe_code` where the rest of the workspace denies
//! it. A malformed file is reported as `0`, never as a panic.
#![allow(unsafe_code)] // the C ABI entry points are exported directly.

use std::cell::RefCell;

use pdf2::Document;

thread_local! {
    static INPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserve `len` bytes for the PDF and return a pointer the host writes to.
///
/// Any previous contents are dropped; the buffer is zero-filled so the host can
/// copy into it without worrying about stale bytes.
#[no_mangle]
pub extern "C" fn pdf2_input(len: usize) -> *mut u8 {
    INPUT.with(|input| {
        let mut input = input.borrow_mut();
        input.clear();
        input.resize(len, 0);
        input.as_mut_ptr()
    })
}

/// Page count of the PDF in the input buffer, or `0` when it cannot be read.
#[no_mangle]
pub extern "C" fn pdf2_pages(len: usize) -> u32 {
    INPUT.with(|input| {
        let input = input.borrow();
        let Some(bytes) = input.get(..len) else {
            return 0;
        };
        match Document::open(bytes).and_then(|document| document.page_count()) {
            Ok(pages) => u32::try_from(pages).unwrap_or(0),
            Err(_) => 0,
        }
    })
}

/// Extract the first page's text into the output buffer and return its length,
/// or `0` when the file cannot be read or interpreted.
#[no_mangle]
pub extern "C" fn pdf2_text(len: usize) -> u32 {
    INPUT.with(|input| {
        let input = input.borrow();
        let Some(bytes) = input.get(..len) else {
            return 0;
        };
        let Ok(document) = Document::open(bytes) else {
            return 0;
        };
        let Ok(page) = document.page(0) else {
            return 0;
        };
        let Ok(text) = document.extract_text(&page) else {
            return 0;
        };
        OUTPUT.with(|output| {
            let mut output = output.borrow_mut();
            output.clear();
            output.extend_from_slice(text.as_bytes());
            u32::try_from(output.len()).unwrap_or(0)
        })
    })
}

/// Pointer to the output buffer filled by [`pdf2_text`].
#[no_mangle]
pub extern "C" fn pdf2_output_ptr() -> *const u8 {
    OUTPUT.with(|output| output.borrow().as_ptr())
}

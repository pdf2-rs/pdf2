//! Run the library under WASI: `pdf2-wasi-smoke [file]`.
//!
//! With no argument the committed `text.pdf` fixture is compiled in, so the
//! smoke test runs without a preopened directory; with a path it goes through
//! `std::fs`, which is what proves the file-system layer works under WASI when
//! the host preopens a directory.

use std::process::ExitCode;

use pdf2::Document;

const EMBEDDED: &[u8] = include_bytes!("../../../../tests/fixtures/text.pdf");

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let bytes = match std::env::args().nth(1) {
        Some(path) => std::fs::read(&path).map_err(|error| format!("{path}: {error}"))?,
        None => EMBEDDED.to_vec(),
    };
    let document = Document::open(bytes).map_err(|error| error.to_string())?;
    let pages = document.page_count().map_err(|error| error.to_string())?;
    println!("wasi: {pages} page(s)");
    let page = document.page(0).map_err(|error| error.to_string())?;
    let text = document
        .extract_text(&page)
        .map_err(|error| error.to_string())?;
    println!("text: {}", text.replace('\n', "\\n"));
    Ok(())
}

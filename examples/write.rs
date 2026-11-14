//! Write a small PDF 2.0 file and read it back.
//!
//! Run with `cargo run --example write`; it writes `hello.pdf` next to the
//! working directory.

use pdf2::{Document, Rect};

fn main() -> Result<(), pdf2::Error> {
    let mut document = Document::new();
    document.add_page(
        Rect::new(0.0, 0.0, 612.0, 792.0),
        "BT /F1 24 Tf 72 700 Td (Hello from pdf2) Tj ET",
    );
    let bytes = document.save()?;
    std::fs::write("hello.pdf", &bytes)?;

    let reopened = Document::open(bytes)?;
    let page = reopened.page(0)?;
    println!(
        "hello.pdf: PDF {}, {} page(s), {}",
        reopened.version(),
        reopened.page_count()?,
        reopened.extract_text(&page)?
    );
    Ok(())
}

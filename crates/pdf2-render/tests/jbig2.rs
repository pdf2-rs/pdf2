//! `/JBIG2Decode` (7.4.7): bi-level scans, behind the `jbig2` feature.
//!
//! The two pdf.js pages are the same 399×400 crop of scanned text, encoded twice:
//! `bitmap-composite-and-xnor.pdf` carries its segments inline, and
//! `bitmap-p32-eof.pdf` keeps them in a `/JBIG2Globals` stream — which is the
//! clause's separate-segments case and the reason a page wired only to
//! `Page::new_embedded(data, None, …)` fails on it.
//!
//! What is proven here: both pages paint, the globals page is not blank, the two
//! decode to the **same pixels** (so the shared dictionaries are really applied),
//! and the sense is ink on white rather than its negative — `hayro-jbig2` reports
//! `black` while a 1-bit PDF sample is 1 for white.

#![cfg(feature = "jbig2")]

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn corpus(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/tier-a")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn render(name: &str) -> RenderedPage {
    let document = Document::open(corpus(name)).expect("open");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

/// Black and white counts for a rendered page.
fn ink(rendered: &RenderedPage) -> (usize, usize) {
    let mut black = 0;
    let mut white = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            match rendered.canvas.pixel(x, y) {
                Some([0, 0, 0, 255]) => black += 1,
                Some([255, 255, 255, 255]) => white += 1,
                Some(other) => panic!("a bi-level page painted {other:?}"),
                None => panic!("({x}, {y}) is outside the canvas"),
            }
        }
    }
    (black, white)
}

#[test]
fn the_inline_page_paints_ink_on_white() {
    let rendered = render("bitmap-composite-and-xnor.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the image draws instead of being reported: {}",
        rendered.report
    );
    let (black, white) = ink(&rendered);
    assert_eq!(black + white, 399 * 400, "every pixel is one of the two");
    assert!(black > 5_000, "the page has ink: {black} black pixels");
    assert!(
        white > black * 4,
        "the sense is ink on white, not the negative: {black} black against {white} white"
    );
}

#[test]
fn the_globals_page_paints_the_same_pixels() {
    let inline = render("bitmap-composite-and-xnor.pdf");
    let globals = render("bitmap-p32-eof.pdf");
    assert_eq!(
        globals.report.to_string(),
        "all operators supported",
        "the globals page draws: {}",
        globals.report
    );
    let (black, white) = ink(&globals);
    assert!(black > 5_000, "the globals page is not blank: {black}");
    assert!(white > black * 4, "and it is ink on white too");
    // The pair is one image encoded two ways, so the shared dictionaries must
    // produce the same raster: a page that ignored `/JBIG2Globals` could still
    // paint something, but not this.
    for y in 0..inline.canvas.height() {
        for x in 0..inline.canvas.width() {
            assert_eq!(
                inline.canvas.pixel(x, y),
                globals.canvas.pixel(x, y),
                "the two encodings disagree at ({x}, {y})"
            );
        }
    }
}

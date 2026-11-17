//! Tiling patterns (`/PatternType 1`): the report before the paint.
//!
//! The corpus page that fills with a tile is `colorkeymask.pdf`, and until a
//! lattice exists it must **say** so rather than fill with the previous colour
//! and claim full support — the difference between a rendered page and a page
//! that lies about being rendered.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn corpus(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/tier-a")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error} — restore the corpus with `cargo run -q -p xtask -- corpus`",
            path.display()
        )
    })
}

fn render(name: &str) -> RenderedPage {
    let document = Document::open(corpus(name)).expect("open corpus file");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

#[test]
fn a_tiling_pattern_is_reported_rather_than_silently_ignored() {
    let rendered = render("colorkeymask.pdf");
    let report = rendered.report.to_string();
    assert!(
        report.contains("pattern"),
        "the fill reports the tiling pattern it cannot paint yet: {report}"
    );
    assert_ne!(
        report, "all operators supported",
        "the page used to claim full support while painting the previous colour"
    );
}

#[test]
fn a_pattern_inside_a_type3_charproc_is_reported_too() {
    // Correction to the plan's research: this page does *not* merely own a
    // `/PatternType 1` object — one of its Type 3 char procedures selects it
    // (`/Pattern cs /P1 scn`), so the same silence hid a pattern fill inside a
    // glyph. It reports now, which also means the lattice round must work
    // through `run_type3_glyph`'s nested walk, not only on page content.
    let rendered = render("ContentStreamCycleType3insideType3.pdf");
    let report = rendered.report.to_string();
    assert!(
        report.contains("pattern"),
        "the char procedure's pattern fill is reported: {report}"
    );
}

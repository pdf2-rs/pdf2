//! What the renderer says about a page, against what it actually drew.
//!
//! Two failure modes are worse than a missing feature because a reader cannot
//! see them: a page that is blank and claims to be fully supported, and a font
//! the renderer silently cannot decode. Both are measured in
//! `docs/RENDERING-QUALITY.md`; the corpus file the second one comes from is
//! pdf.js's, so the test skips with an instruction when the corpus is absent
//! (it is committed, and `cargo run -q -p xtask -- corpus` restores it).

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn render(bytes: Vec<u8>, page: usize) -> RenderedPage {
    let document = Document::open(bytes).expect("open");
    render_page_number(&document, page, &RenderOptions::default()).expect("render")
}

fn ink(rendered: &RenderedPage) -> usize {
    let white = [255_u8, 255, 255, 255];
    let mut count = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if rendered.canvas.pixel(x, y) != Some(white) {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn a_page_that_paints_nothing_visible_says_so() {
    // `1 1 1 rg … re f` over a white background: every operator is supported and
    // the raster is empty, which used to read as "all operators supported".
    let rendered = render(
        std::fs::read(repo("tests/fixtures/invisible-ink.pdf")).expect("fixture"),
        0,
    );
    assert_eq!(ink(&rendered), 0, "white on white leaves no ink");
    assert_eq!(
        rendered.report.to_string(),
        "drew-nothing×1",
        "an empty page that painted is reported as such"
    );
    assert!(rendered.canvas.is_blank());
}

#[test]
fn a_genuinely_blank_page_is_not_called_drew_nothing() {
    // An empty content stream is a blank page by design, and saying otherwise
    // would be noise on every divider page in every document.
    for page in 0..2 {
        let rendered = render(
            std::fs::read(repo("tests/fixtures/blank-page.pdf")).expect("fixture"),
            page,
        );
        assert!(rendered.canvas.is_blank(), "page {page} is blank by design");
        assert_eq!(
            rendered.report.to_string(),
            "all operators supported",
            "page {page}: a blank page is not a failure"
        );
    }
}

#[test]
fn a_font_program_without_a_cmap_is_named_rather_than_guessed() {
    let path = repo("corpus/tier-a/TrueType_without_cmap.pdf");
    let Ok(bytes) = std::fs::read(&path) else {
        panic!(
            "{}: restore the corpus with `cargo run -q -p xtask -- corpus`",
            path.display()
        );
    };
    let rendered = render(bytes, 0);
    assert_eq!(
        rendered.report.to_string(),
        "font:TrueType-without-cmap×1",
        "the page names the reason instead of claiming support"
    );
}

//! CFF and Type 1 outlines, against the corpus files that carry them.
//!
//! The corpus is committed in the repository (and excluded from the published
//! crate), so these tests run where the renderer's own CI runs and skip nowhere
//! silently: a missing corpus file is a failure with the command that restores
//! it, not a passing test.
//!
//! What is proven here: a `/CIDFontType0` font no longer reports a missing font
//! family, and its glyphs are ink on the canvas. The two corpus pages are the
//! ones `docs/ROADMAP-NEXT.md` item 4 names; their oracle cases wait for the
//! operators items 10 and 10b own.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

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
fn a_cid_cff_font_draws_its_outlines() {
    // `/Subtype /CIDFontType0C`: a bare CFF program with no sfnt container.
    let rendered = render("cid_cff.pdf");
    let report = rendered.report.to_string();
    assert!(
        !report.contains("font:CIDFontType0"),
        "the CFF font is reported missing: {report}"
    );
    assert!(
        !report.contains("font:"),
        "no font family is missing: {report}"
    );
    // The page is text over white: outlines, not a full-page fill.
    let ink = ink(&rendered);
    assert!(ink > 500, "the glyphs are ink: {ink} pixels");
    assert!(
        ink < rendered.canvas.width() as usize * rendered.canvas.height() as usize / 4,
        "the page is mostly white: {ink} pixels"
    );
}

#[test]
fn an_open_type_cff_font_draws_the_glyph_its_charset_names() {
    // `/Subtype /OpenType` around a CID-keyed CFF whose charter maps CID 0xA911
    // to glyph 1: using the CID as a glyph id would draw nothing.
    let rendered = render("Embedded_font.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the page is inside the supported surface"
    );
    assert!(ink(&rendered) > 200, "the glyph is drawn");
}

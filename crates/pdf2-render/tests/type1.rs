//! Type 1 fonts (`/FontFile`): the program, its `eexec` section and its
//! charstrings.
//!
//! `ttf-parser` reads sfnt and CFF; a Type 1 program is PostScript, so the
//! renderer parses it itself (`crates/pdf2-render/src/type1.rs`). No corpus page
//! carries one — verified across `corpus/tier-a` — so the proof is the generated
//! fixture: three glyphs drawn with different operator sets, one of them through
//! `callsubr`, each inside its own advance width.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

fn fixture(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn render(bytes: Vec<u8>) -> RenderedPage {
    let document = Document::open(bytes).expect("open fixture");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

#[test]
fn a_type1_font_draws_its_charstrings() {
    let rendered = render(fixture("type1.pdf"));
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the /FontFile program loads and draws: {}",
        rendered.report
    );
    // "ABC" set at 90 pt: each glyph occupies its own advance band, so three
    // bands with ink mean three glyphs — a decoder that lost a glyph would leave
    // one band empty, and one that misplaced them would put ink in the wrong one.
    let ink_in = |from: u32, to: u32| {
        let mut painted = 0;
        for y in 0..rendered.canvas.height() {
            for x in from..to.min(rendered.canvas.width()) {
                if rendered.canvas.pixel(x, y) != Some([255_u8, 255, 255, 255]) {
                    painted += 1;
                }
            }
        }
        painted
    };
    // 90 pt at 72 dpi: one glyph unit is 0.09 px, so the 600-unit advance is 54 px.
    let bands = [(40, 94), (94, 130), (130, 157)];
    for (index, (from, to)) in bands.iter().enumerate() {
        let painted = ink_in(*from, *to);
        assert!(
            painted > 100,
            "glyph {} has ink: {painted} pixels",
            index + 1
        );
    }
}

#[test]
fn a_type1_font_without_a_usable_program_is_reported_not_guessed() {
    // The same page with the program replaced by bytes that are not a font: the
    // renderer must fall back and say so, rather than draw nothing silently.
    let mut bytes = fixture("type1.pdf");
    let at = bytes
        .windows(b"%!PS-AdobeFont-1.0".len())
        .position(|window| window == b"%!PS-AdobeFont-1.0")
        .expect("header");
    // The header and the `eexec` keyword: without the latter there is no
    // encrypted section to read, which is what "not a Type 1 program" looks like.
    let end = (at + 400).min(bytes.len());
    for byte in &mut bytes[at..end] {
        *byte = b'x';
    }
    let rendered = render(bytes);
    assert!(
        rendered.report.to_string().contains("font:FontFile"),
        "a broken program is reported: {}",
        rendered.report
    );
}

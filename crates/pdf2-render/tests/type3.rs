//! Type 3 fonts: glyphs that are content streams, not outlines.
//!
//! The two corpus pages are the ones `docs/ROADMAP-NEXT.md` item 4 names. What is
//! proven here: a char procedure runs in glyph space (so `/FontMatrix` places it),
//! the glyph's own paint arrives on the canvas in its own colours, and a font whose
//! glyph shows more text in the same font stops at the depth limit instead of
//! recursing forever.

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

fn count(rendered: &RenderedPage, matches: impl Fn([u8; 4]) -> bool) -> usize {
    let mut painted = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if let Some(pixel) = rendered.canvas.pixel(x, y)
                && matches(pixel)
            {
                painted += 1;
            }
        }
    }
    painted
}

#[test]
fn type3_glyph_procedures_paint_their_own_content() {
    let rendered = render("Type3WordSpacing.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "every operator of the page and its char procedures is honoured: {}",
        rendered.report
    );
    // The page's glyphs are a blue triangle and a red rectangle outline drawn by
    // the char procedures themselves — ink the outline path could not produce.
    let blue = count(&rendered, |[r, g, b, _]| r < 80 && g < 80 && b > 150);
    // The markers are thin and dashed, so anti-aliasing leaves them tinted rather
    // than saturated; "reddish" is the test, not one exact value.
    let red = count(&rendered, |[r, g, b, _]| {
        i32::from(r) > i32::from(g) + 40 && i32::from(r) > i32::from(b) + 40
    });
    assert!(blue > 100, "the triangles are filled: {blue} blue pixels");
    assert!(red > 20, "the markers are stroked: {red} red pixels");
}

#[test]
fn a_type3_glyph_that_shows_text_in_its_own_font_terminates() {
    // `ContentStreamCycleType3insideType3.pdf`: a glyph whose procedure shows
    // text in the same font. The depth limit is what makes this terminate; the
    // test is that it renders at all, and that the inner glyph's paint is there.
    let rendered = render("ContentStreamCycleType3insideType3.pdf");
    // The glyph's procedure also fills with a `/PatternType 1` tile; it paints
    // since the lattice landed, and the recursion still terminates — which is what
    // this test is about (`tests/pattern.rs` covers the tile itself).
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the nested glyph is drawn, its tile painted: {}",
        rendered.report
    );
    let green = count(&rendered, |[r, g, b, _]| g > 150 && r < 100 && b < 100);
    assert!(
        green > 50,
        "the inner triangle is drawn: {green} green pixels"
    );
}

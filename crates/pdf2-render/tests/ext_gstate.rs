//! `/ExtGState` (8.4.5): constant alpha, the line state, and the honesty rule for
//! the parts this round does not implement.
//!
//! `tests/fixtures/alpha.pdf` is drawn so the arithmetic is checkable: 50 % red
//! over white is `(255, 128, 128)`, because the canvas blends
//! `source·alpha + destination·(1-alpha)`. A state that asks for something the
//! renderer cannot honour — a soft mask, a non-normal blend mode — keeps its `gs`
//! note instead of disappearing: silent success is the failure mode this codebase
//! keeps having to fix.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn render(path: &str) -> RenderedPage {
    let bytes = std::fs::read(repo(path)).expect("fixture");
    let document = Document::open(bytes).expect("open");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("inside the canvas")
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const PINK: [u8; 4] = [255, 128, 128, 255];
const BLUE_HALF: [u8; 4] = [128, 128, 255, 255];

#[test]
fn constant_alpha_reaches_the_fill() {
    let rendered = render("tests/fixtures/alpha.pdf");
    // `/ca 0.5` red over white.
    assert_eq!(pixel(&rendered, 50, 150), PINK);
    // The next `q` restores alpha to 1, so the same red is solid.
    assert_eq!(pixel(&rendered, 150, 150), RED);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "every state in this fixture is honoured: {}",
        rendered.report
    );
}

#[test]
fn constant_alpha_reaches_the_stroke() {
    let rendered = render("tests/fixtures/alpha.pdf");
    assert_eq!(pixel(&rendered, 100, 80), BLUE_HALF, "`/CA 0.5` blue");
}

#[test]
fn the_line_state_comes_from_the_dictionary() {
    let rendered = render("tests/fixtures/alpha.pdf");
    // `/D [[12 12] 0]`, `/LW 6`, butt caps on the top line: ink from x = 20 to
    // 32, a gap to 44, ink again. Round caps would close a 3/3 pattern entirely,
    // which is how this fixture first read as solid.
    assert_eq!(pixel(&rendered, 25, 40), BLACK);
    assert_eq!(pixel(&rendered, 38, 40), WHITE, "the dash gap");
    assert_eq!(pixel(&rendered, 50, 40), BLACK, "the next dash");
}

#[test]
fn a_soft_mask_is_still_reported() {
    // `cmykjpeg.pdf`'s state carries `/SMask`; honouring `/ca` must not make the
    // operator look supported when the mask is not applied.
    let rendered = render("corpus/tier-a/cmykjpeg.pdf");
    assert!(
        rendered.report.to_string().contains("gs"),
        "the soft mask keeps the note: {}",
        rendered.report
    );
}

#[test]
fn a_blend_mode_that_is_not_normal_is_still_reported() {
    // `blendmode.pdf` sets all sixteen; until they land the note stands.
    let rendered = render("corpus/tier-a/blendmode.pdf");
    assert!(
        rendered.report.to_string().contains("gs"),
        "blend modes keep the note: {}",
        rendered.report
    );
}

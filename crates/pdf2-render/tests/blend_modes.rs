//! The sixteen `/BM` blend modes (11.3.5, Table 136).
//!
//! `tests/fixtures/blend-modes.pdf` paints one 40 × 40 cell per mode over a 50 %
//! grey backdrop, with the source colour `(0.8, 0.2, 0.4)`. Several results then
//! follow by arithmetic rather than by taste: Multiply is `(0.4, 0.1, 0.2)`,
//! Screen `(0.9, 0.6, 0.7)`, Difference `(0.3, 0.3, 0.1)`.
//!
//! Values are asserted within ±2 per channel: the backdrop is `0.5` stored as a
//! byte (128/255 = 0.5019), so the exact integer depends on that rounding — the
//! formula being checked is the float one.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

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

/// The centre of the cell for `index` in a 4 × 4 grid of 40 px cells.
fn cell(index: u32) -> (u32, u32) {
    let column = index % 4;
    let row = index / 4;
    (10 + column * 45 + 20, 10 + row * 45 + 20)
}

fn close(actual: [u8; 4], expected: [u8; 3]) -> bool {
    (0..3).all(|channel| i32::from(actual[channel]) - i32::from(expected[channel]).abs() <= 2)
}

fn assert_cell(rendered: &RenderedPage, index: u32, expected: [u8; 3], what: &str) {
    let (x, y) = cell(index);
    let actual = rendered.canvas.pixel(x, y).expect("inside the canvas");
    assert!(
        close(actual, expected),
        "{what} at cell {index}: got {actual:?}, expected about {expected:?}"
    );
}

#[test]
fn all_sixteen_modes_are_honoured() {
    let rendered = render("tests/fixtures/blend-modes.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "no `gs` note is left for any of the sixteen: {}",
        rendered.report
    );
}

#[test]
fn the_separable_modes_follow_their_formulae() {
    let rendered = render("tests/fixtures/blend-modes.pdf");
    // Source (204, 51, 102) over the grey backdrop.
    assert_cell(&rendered, 0, [204, 51, 102], "Normal");
    assert_cell(&rendered, 1, [102, 26, 51], "Multiply: backdrop × source");
    assert_cell(&rendered, 2, [230, 153, 179], "Screen: 1-(1-b)(1-s)");
    assert_cell(&rendered, 4, [128, 51, 102], "Darken: per-channel minimum");
    assert_cell(
        &rendered,
        5,
        [204, 128, 128],
        "Lighten: per-channel maximum",
    );
    assert_cell(
        &rendered,
        10,
        [76, 77, 26],
        "Difference: |backdrop - source|",
    );
}

#[test]
fn a_non_separable_mode_keeps_the_backdrops_luminance() {
    // `/Luminosity` takes the source's luminance and the backdrop's hue and
    // saturation — a grey backdrop means all three channels come out equal.
    let rendered = render("tests/fixtures/blend-modes.pdf");
    let (x, y) = cell(15);
    let pixel = rendered.canvas.pixel(x, y).expect("inside the canvas");
    assert!(
        pixel[0] == pixel[1] && pixel[1] == pixel[2],
        "luminosity over grey is grey: {pixel:?}"
    );
}

#[test]
fn the_corpus_blend_page_loses_its_gs_note() {
    // `blendmode.pdf` sets every one of the sixteen; M5's last clause.
    let rendered = render("corpus/tier-a/blendmode.pdf");
    assert!(
        !rendered.report.to_string().contains("gs"),
        "every blend mode is honoured: {}",
        rendered.report
    );
}

//! Annotation appearance streams (12.5.5): a page whose `/Contents` is empty and
//! whose only visible content is `/Annots → /AP → /N`.
//!
//! Measured 2026-11-15: 25 of the 49 sample pages that were blank *and reported
//! nothing* have real paint operators in their file, and the three inspected are
//! this shape (`docs/RENDERING-QUALITY.md`). `tests/fixtures/annotations.pdf`
//! carries one annotation per rule: a `/Matrix` inside the appearance, an
//! `/AP /N` dictionary keyed by `/AS`, and a hidden one that must not draw.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn render(fixture: &str) -> RenderedPage {
    let bytes = std::fs::read(repo(&format!("tests/fixtures/{fixture}"))).expect("fixture");
    let document = Document::open(bytes).expect("open");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("inside the canvas")
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];

#[test]
fn a_page_whose_content_is_one_appearance_stream_is_not_blank() {
    let rendered = render("annotations.pdf");
    // `/Rect [50 50 100 100]`, appearance `/BBox [0 0 10 10]` with
    // `/Matrix [2 0 0 2 0 0]` painting a 5 × 5 square: the transformed box maps
    // onto the rectangle, and the square keeps its quarter share of it.
    assert_eq!(pixel(&rendered, 62, 137), RED);
    assert_eq!(pixel(&rendered, 20, 137), WHITE, "left of the rectangle");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the appearance was drawn, not reported: {}",
        rendered.report
    );
}

#[test]
fn a_state_dictionary_picks_the_as_entry() {
    // `/AP /N << /On 6 0 R /Off 7 0 R >> /AS /On`: green, not the grey `/Off`.
    let rendered = render("annotations.pdf");
    assert_eq!(pixel(&rendered, 145, 125), GREEN);
}

#[test]
fn a_hidden_annotation_draws_nothing() {
    // `/F 2` is the hidden flag (12.5.3): its blue square must never appear.
    let rendered = render("annotations.pdf");
    assert_eq!(pixel(&rendered, 75, 55), WHITE);
}

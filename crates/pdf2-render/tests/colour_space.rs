//! Paint colour spaces: an `/ICCBased` array resolves, and one that cannot is named.
//!
//! The CV that started this work painted **black** because its fill space was
//! `[/ICCBased 20 0 R]` and the resolver assumed every array was a `/Separation`
//! (alternate at index 2, a function last). The space failed, `select_colour_space`
//! swallowed the error, and `sc` left the fill at its initial black — so the report
//! named no space and the page said nothing was wrong. These three pages pin the
//! three outcomes: resolved by `/N`, resolved by `/Alternate`, and reported by name.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn render(page: usize) -> RenderedPage {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/icc-paint.pdf");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let document = Document::open(bytes).expect("open fixture");
    render_page_number(&document, page, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("in bounds")
}

#[test]
fn an_icc_based_paint_space_resolves_by_its_component_count() {
    let rendered = render(0);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the space resolves, so nothing is reported: {}",
        rendered.report
    );
    // `1 1 1 sc` in a three-component profile is white, and `0 0 0 sc` is black —
    // the CV's page was black everywhere because neither reached the canvas.
    assert_eq!(
        pixel(&rendered, 1, 1),
        [255, 255, 255, 255],
        "the fill is white"
    );
    assert_eq!(pixel(&rendered, 50, 50), [0, 0, 0, 255], "the bar is black");
}

#[test]
fn an_icc_based_space_without_n_falls_back_to_its_alternate() {
    // `8.6.5.5`: `/Alternate` is what a reader that cannot honour the profile should
    // use, and a profile with no `/N` is exactly that case.
    let rendered = render(1);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "/Alternate resolves the space: {}",
        rendered.report
    );
    assert_eq!(
        pixel(&rendered, 50, 50),
        [0, 0, 0, 255],
        "`0 0 0 sc` is black"
    );
}

#[test]
fn a_space_that_cannot_be_resolved_is_named() {
    // The old behaviour was silence: `sc` kept whatever colour was there before and
    // the page gave no hint. Now the report carries the space's own name.
    let rendered = render(2);
    let report = rendered.report.to_string();
    assert!(
        report.contains("cs:Cs3"),
        "the unresolvable space is reported by name: {report}"
    );
}

//! Form XObjects (8.10): a page's content is often not in its `/Contents` at all
//! but in a `/Subtype /Form` stream — 39.4 % of the pages in the measured sample
//! report `Do` without them (`docs/RENDERING-QUALITY.md`).
//!
//! `tests/fixtures/forms.pdf` carries one form per rule: a `/Matrix` that moves
//! and scales, a `/BBox` smaller than what its content paints, a form with no
//! `/Resources` of its own, and a form drawing a form.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

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
const BLUE: [u8; 4] = [0, 0, 255, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];

#[test]
fn a_form_with_a_matrix_lands_where_its_matrix_says() {
    let rendered = render("forms.pdf");
    // `/F1`: `/Matrix [2 0 0 2 20 20]` over a red square at form (0,0)–(25,25),
    // so user space (20,20)–(70,70) and device rows 130–180.
    assert_eq!(pixel(&rendered, 45, 155), RED);
    assert_eq!(pixel(&rendered, 5, 155), WHITE, "outside the form's matrix");
}

#[test]
fn a_form_is_clipped_by_its_bbox_not_by_the_canvas() {
    let rendered = render("forms.pdf");
    // `/F2` paints 120 × 120 but declares `/BBox [0 0 40 40]`.
    assert_eq!(pixel(&rendered, 20, 180), GREEN);
    assert_eq!(
        pixel(&rendered, 60, 190),
        WHITE,
        "content past the /BBox is cut"
    );
}

#[test]
fn a_form_without_resources_inherits_the_page_dictionary() {
    // `/F3` has no `/Resources` and draws `/Inner`, which only the page declares.
    let rendered = render("forms.pdf");
    assert_eq!(pixel(&rendered, 160, 40), MAGENTA);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "nothing was left undrawn, so no `Do` note: {}",
        rendered.report
    );
}

#[test]
fn a_form_can_draw_another_form() {
    let rendered = render("forms.pdf");
    // `/F4` → `/Deep`, whose own matrix puts a blue square at user (100,20).
    assert_eq!(pixel(&rendered, 110, 170), BLUE);
}

#[test]
fn a_form_cycle_is_reported_rather_than_followed() {
    // A form that draws itself: the depth cap has to stop it, and the page has to
    // say that something was not drawn.
    let rendered = render("form-cycle.pdf");
    assert_eq!(pixel(&rendered, 5, 95), BLUE, "the first level still draws");
    assert_eq!(
        rendered.report.to_string(),
        "Do×1",
        "the level past MAX_FORM_DEPTH is reported once: {}",
        rendered.report
    );
}

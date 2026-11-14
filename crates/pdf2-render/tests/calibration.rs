//! Rendering correctness, page by page, from numbers that come from the
//! specification rather than from a previous render.
//!
//! `tests/fixtures/calibration.pdf` is built by `tests/fixtures/make_fixtures.py`
//! and holds six pages:
//!
//! 1. colour — DeviceGray, DeviceRGB and DeviceCMYK cells whose byte values are
//!    arithmetic;
//! 2. strokes — the three cap styles, a miter and a bevel join, the nonzero and
//!    even-odd fill rules on identical geometry, a dash phase, and a Bezier
//!    circle whose area is pi r^2;
//! 3. images — a 2x2 checkerboard at integer scale (exact block edges), the
//!    same image inverted by `/Decode`, gray and `/Indexed` images, and the
//!    image on a quarter-turn matrix;
//! 4. page boxes — a `/MediaBox` with a non-zero origin;
//! 5. `/Rotate 90`;
//! 6. text metrics — horizontal scaling and word spacing measured against the
//!    same string at 100%.
//!
//! Every assertion states where the pixel comes from. A golden PNG pins the
//! whole raster as a regression net, but it cannot tell a right answer from a
//! wrong one; these probes can.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{encode_png, render_page_number, RenderOptions, RenderedPage};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn render(page: usize) -> RenderedPage {
    let bytes = std::fs::read(repo("tests/fixtures/calibration.pdf")).expect("read fixture");
    let document = Document::open(bytes).expect("open calibration.pdf");
    render_page_number(&document, page, &RenderOptions::default()).expect("render page")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 3] {
    let [r, g, b, a] = rendered.canvas.pixel(x, y).expect("inside the canvas");
    assert_eq!(a, 255, "the canvas is opaque");
    [r, g, b]
}

const WHITE: [u8; 3] = [255, 255, 255];
const BLACK: [u8; 3] = [0, 0, 0];
const RED: [u8; 3] = [255, 0, 0];
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const CYAN: [u8; 3] = [0, 255, 255];
const MAGENTA: [u8; 3] = [255, 0, 255];
const YELLOW: [u8; 3] = [255, 255, 0];

#[track_caller]
fn expect_at(rendered: &RenderedPage, x: u32, y: u32, colour: [u8; 3], what: &str) {
    assert_eq!(pixel(rendered, x, y), colour, "{what} at ({x}, {y})");
}

/// Non-background pixels in a device-space box, for area and extent probes.
fn ink(rendered: &RenderedPage, x0: u32, y0: u32, x1: u32, y1: u32) -> usize {
    let mut count = 0;
    for y in y0..y1 {
        for x in x0..x1 {
            if pixel(rendered, x, y) != WHITE {
                count += 1;
            }
        }
    }
    count
}

/// Rightmost inked column in a band, or `None` when the band is empty.
fn right_edge(rendered: &RenderedPage, x0: u32, y0: u32, x1: u32, y1: u32) -> Option<u32> {
    let mut edge = None;
    for x in x0..x1 {
        for y in y0..y1 {
            if pixel(rendered, x, y) != WHITE {
                edge = Some(x);
                break;
            }
        }
    }
    edge
}

#[test]
fn colour_operators_produce_the_arithmetic_byte_values() {
    let page = render(0);
    assert_eq!((page.canvas.width(), page.canvas.height()), (200, 200));

    // DeviceGray: 0, 0.5, 0.25, 0.75 scaled by 255 and rounded.
    expect_at(&page, 30, 30, BLACK, "gray 0");
    expect_at(&page, 76, 30, [128, 128, 128], "gray 0.5");
    expect_at(&page, 122, 30, [64, 64, 64], "gray 0.25");
    expect_at(&page, 168, 30, [191, 191, 191], "gray 0.75");

    // DeviceRGB: components are already the output bytes.
    expect_at(&page, 30, 76, RED, "rgb 1 0 0");
    expect_at(&page, 76, 76, GREEN, "rgb 0 1 0");
    expect_at(&page, 122, 76, BLUE, "rgb 0 0 1");
    expect_at(&page, 168, 76, [51, 102, 255], "rgb 0.2 0.4 1");

    // DeviceCMYK with no colour space: R = (1-c)(1-k) per component.
    expect_at(&page, 30, 122, CYAN, "cmyk 1 0 0 0");
    expect_at(&page, 76, 122, [128, 128, 128], "cmyk 0 0 0 0.5");
    expect_at(&page, 122, 122, RED, "cmyk 0 1 1 0");
    expect_at(&page, 168, 122, BLACK, "cmyk 1 1 0 1");

    // The gutters stay background: the cells really are cells.
    expect_at(&page, 50, 30, WHITE, "gutter between cells");
    assert_eq!(page.report.unsupported_total(), 0, "{}", page.report);
}

#[test]
fn cap_styles_extend_by_half_the_line_width_in_their_own_way() {
    // Width 12 lines from x = 40 to x = 140: butt stops at 140, square and
    // round reach 146, and only the square fills the corner past the end.
    // Device y is 300 - user y; the three lines sit at user y 260, 230, 200,
    // and a probe names the pixel whose centre (x + 0.5, y + 0.5) it tests.
    let page = render(1);
    assert_eq!((page.canvas.width(), page.canvas.height()), (300, 300));

    expect_at(&page, 139, 40, BLACK, "butt: inside the end");
    expect_at(
        &page,
        140,
        40,
        WHITE,
        "butt: the pixel past x = 140 is empty",
    );
    expect_at(&page, 143, 70, BLACK, "round: past the end is painted");
    expect_at(
        &page,
        145,
        73,
        WHITE,
        "round: outside its radius, 6.5 px out",
    );
    expect_at(
        &page,
        145,
        104,
        BLACK,
        "square: fills the corner at (5.5, 3.5)",
    );
    expect_at(&page, 146, 100, WHITE, "square: stops at half the width");
}

#[test]
fn joins_are_mitered_or_bevelled() {
    // Right-angle corners at user (80, 120) with a miter join and (180, 120)
    // with a bevel: only the miter fills the square corner outside the turn.
    // Device (83, 175) sits inside the miter's triangle but outside both
    // strokes, so a round or beveled join leaves it empty.
    let page = render(1);
    expect_at(&page, 83, 175, BLACK, "miter fills the corner");
    expect_at(&page, 183, 175, WHITE, "bevel cuts the corner");
    expect_at(&page, 75, 177, BLACK, "both paint the stroke itself");
    expect_at(&page, 175, 177, BLACK, "both paint the stroke itself");
    expect_at(&page, 83, 185, BLACK, "the inside of the turn is covered");
}

#[test]
fn the_fill_rule_decides_whether_the_inner_subpath_is_a_hole() {
    // The same two nested rectangles, both wound the same way: nonzero fills
    // the inner one, even-odd punches it out.
    let page = render(1);
    expect_at(&page, 50, 250, RED, "nonzero keeps the inner rectangle");
    expect_at(&page, 30, 250, RED, "and the ring around it");
    expect_at(
        &page,
        170,
        250,
        WHITE,
        "even-odd punches the inner rectangle",
    );
    expect_at(&page, 150, 250, BLUE, "the ring is still painted");
}

#[test]
fn a_dash_phase_starts_the_pattern_partway_through_a_dash() {
    // `[20 10] 5 d` from x = 120: 5 of the 20 are already spent, so the first
    // dash covers 120..135, the gap 135..145, the next dash 145..165.
    let page = render(1);
    expect_at(&page, 125, 140, MAGENTA, "inside the first, shortened dash");
    expect_at(&page, 140, 140, WHITE, "inside the first gap");
    expect_at(&page, 150, 140, MAGENTA, "inside the second, full dash");
    expect_at(&page, 170, 140, WHITE, "inside the second gap");
}

#[test]
fn a_bezier_circle_covers_about_pi_r_squared() {
    // Four cubic arcs of radius 30 at user (250, 230), filled: 2827 device
    // pixels at 1 px per point. The approximation and antialiasing are worth a
    // few percent, not a factor of two.
    let page = render(1);
    expect_at(&page, 250, 70, [0, 153, 0], "the centre is filled");
    expect_at(&page, 250, 34, WHITE, "outside the top of the circle");
    let area = ink(&page, 215, 35, 285, 105);
    let expected = std::f64::consts::PI * 30.0 * 30.0;
    let ratio = area as f64 / expected;
    assert!(
        (0.96..=1.04).contains(&ratio),
        "ink {area} vs pi r^2 {expected:.0} (ratio {ratio:.3})"
    );
}

#[test]
fn an_image_at_integer_scale_keeps_exact_block_edges() {
    // A 2x2 checkerboard drawn over 120 pt: red, green / blue, white, each a
    // 60 px block with the boundary exactly at the odd/even pixel pair.
    let page = render(2);
    assert_eq!((page.canvas.width(), page.canvas.height()), (400, 300));

    expect_at(&page, 50, 180, RED, "top-left block");
    expect_at(&page, 110, 180, GREEN, "top-right block");
    expect_at(&page, 50, 260, BLUE, "bottom-left block");
    expect_at(&page, 110, 260, WHITE, "bottom-right block");
    expect_at(&page, 79, 180, RED, "last pixel before the vertical edge");
    expect_at(&page, 81, 180, GREEN, "first pixel after the vertical edge");
    expect_at(&page, 50, 219, RED, "last pixel before the horizontal edge");
    expect_at(
        &page,
        50,
        221,
        BLUE,
        "first pixel after the horizontal edge",
    );
}

#[test]
fn decode_inverts_and_indexed_picks_palette_entries() {
    let page = render(2);
    // /Decode [1 0 1 0 1 0]: every component is complemented.
    expect_at(&page, 50, 50, CYAN, "inverted red");
    expect_at(&page, 110, 50, MAGENTA, "inverted green");
    expect_at(&page, 50, 110, YELLOW, "inverted blue");
    expect_at(&page, 110, 110, BLACK, "inverted white");

    // DeviceGray, one black and one white sample.
    expect_at(&page, 175, 250, BLACK, "gray image, first sample");
    expect_at(&page, 205, 250, WHITE, "gray image, second sample");

    // /Indexed with the palette red, blue.
    expect_at(&page, 255, 250, RED, "indexed 0 is red");
    expect_at(&page, 285, 250, BLUE, "indexed 1 is blue");
    expect_at(&page, 235, 250, WHITE, "outside the indexed image");
}

#[test]
fn a_quarter_turn_matrix_rotates_the_image_without_mirroring_it() {
    // [0 120 -120 0 360 20] maps image space (u, v) to device
    // (360 - 120 v, 20 + 120 u), and the image's first row sits at v = 1. So
    // red (u<0.5, v>0.5) lands top-left of the drawn square, green below it.
    let page = render(2);
    expect_at(&page, 190, 110, RED, "red after a quarter turn");
    expect_at(&page, 190, 50, GREEN, "green after a quarter turn");
    expect_at(&page, 250, 110, BLUE, "blue after a quarter turn");
    expect_at(&page, 250, 50, WHITE, "white after a quarter turn");
    expect_at(&page, 161, 110, RED, "the drawn square starts at x = 160");
    expect_at(&page, 158, 110, WHITE, "and nothing is painted to its left");
}

#[test]
fn a_media_box_origin_that_is_not_zero_shifts_the_content() {
    // /MediaBox [50 60 250 260]: the canvas is 200x200 and user (50, 60) is the
    // bottom-left corner, so that rectangle lands at the top-left of the raster.
    let page = render(3);
    assert_eq!((page.canvas.width(), page.canvas.height()), (200, 200));
    expect_at(&page, 10, 190, RED, "the bottom-left rectangle");
    expect_at(&page, 190, 10, BLUE, "the top-right rectangle");
    expect_at(&page, 100, 100, WHITE, "the space between them");
    expect_at(&page, 0, 0, WHITE, "nothing leaks into the corner");
}

#[test]
fn rotate_90_swaps_the_canvas_and_turns_the_page_clockwise() {
    // /MediaBox [0 0 300 100] with /Rotate 90 is a 100x300 raster. User
    // (x, y) maps to device (y, x), so the rectangle at the user origin sits at
    // the top-left and the one at x = 200 at the bottom-left.
    let page = render(4);
    assert_eq!((page.canvas.width(), page.canvas.height()), (100, 300));
    expect_at(&page, 50, 50, RED, "the rectangle at the user origin");
    expect_at(&page, 50, 250, BLUE, "the rectangle at user x = 200");
    expect_at(&page, 50, 150, WHITE, "the gap between them");
    expect_at(&page, 5, 5, RED, "the turn reaches the corner");
    expect_at(&page, 95, 295, BLUE, "and the far corner");
}

#[test]
fn horizontal_scaling_and_word_spacing_change_the_run_by_the_factor() {
    // The same string at the same size, once at 100% and once at 50% of the
    // horizontal scale: half the width, measured from the same left margin.
    let page = render(5);
    assert_eq!((page.canvas.width(), page.canvas.height()), (300, 200));
    let full = right_edge(&page, 0, 12, 300, 55).expect("the 100% run");
    let half = right_edge(&page, 0, 86, 300, 122).expect("the 50% run");
    let full_width = f64::from(full) - 20.0;
    let half_width = f64::from(half) - 20.0;
    assert!(
        (0.47..=0.53).contains(&(half_width / full_width)),
        "50% run is {half_width} wide against {full_width} at 100%"
    );

    // Two word spaces at `Tw 40`, 20 pt, no horizontal scaling: the run grows
    // by 80 user units.
    let tight = right_edge(&page, 0, 122, 300, 145).expect("the run without word spacing");
    let spaced = right_edge(&page, 0, 162, 300, 185).expect("the run with word spacing");
    let growth = f64::from(spaced) - f64::from(tight);
    assert!(
        (76.0..=84.0).contains(&growth),
        "word spacing moved the right edge by {growth}, expected 2 x 40"
    );
}

#[test]
fn every_calibration_page_uses_only_supported_operators() {
    for page in 0..6 {
        let rendered = render(page);
        assert_eq!(
            rendered.report.unsupported_total(),
            0,
            "page {}: {}",
            page + 1,
            rendered.report
        );
    }
}

#[test]
fn the_committed_goldens_match_the_renderer() {
    for page in 0..6 {
        let rendered = render(page);
        let png = encode_png(&rendered.canvas);
        let golden_path = repo(&format!(
            "tests/fixtures/golden/calibration-{}.png",
            page + 1
        ));
        if std::env::var_os("BLESS").is_some() {
            std::fs::write(&golden_path, &png).expect("bless golden");
            continue;
        }
        let golden = std::fs::read(&golden_path).expect("read golden");
        assert!(
            golden == png,
            "page {} differs from its golden (rendered {} bytes, golden {} bytes); \
             re-run with BLESS=1 if the change is intended",
            page + 1,
            png.len(),
            golden.len()
        );
    }
}

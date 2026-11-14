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
//!    same string at 100%;
//! 7. `/Widths` — three fonts that differ only in the width table, so the
//!    distance between two identical glyphs is the space advance;
//! 8. a Type 0 font, which must be reported rather than painted;
//! 9. an embedded TrueType program written by the generator, whose `A` is a box
//!    and whose `B` is a box with a hole — shapes the bitmap fallback has not;
//! 10. a Type 0 (CID) font over that program with Identity-H and `/CIDToGIDMap`
//!     `/Identity`, drawing `A B A` from two-byte codes;
//! 11. the same with a `/CIDToGIDMap` stream that swaps the two glyphs, so the
//!     same codes draw `B A B`.
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

/// Runs of inked columns in a band, left to right.
fn ink_runs(rendered: &RenderedPage, x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<(u32, u32)> {
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for x in x0..x1 {
        let inked = (y0..y1).any(|y| pixel(rendered, x, y) != WHITE);
        match (inked, runs.last_mut()) {
            (true, Some(last)) if last.1 == x => last.1 = x + 1,
            (true, _) => runs.push((x, x + 1)),
            (false, _) => {}
        }
    }
    runs
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
fn widths_from_the_font_dictionary_set_the_advance() {
    // "I I" at 20 pt on three fonts that differ only in /Widths. The second
    // glyph starts after `I` plus the space, so the distance is the two widths
    // in 1/1000 em: (333 + 250), (333 + 750), and for the third font both `Z`
    // and the space are outside the array and take /MissingWidth 900.
    let page = render(6);
    assert_eq!((page.canvas.width(), page.canvas.height()), (300, 200));
    for (band, expected) in [
        ((40, 55), (333.0 + 250.0) / 1000.0 * 20.0),
        ((90, 105), (333.0 + 750.0) / 1000.0 * 20.0),
        ((140, 155), (900.0 + 900.0) / 1000.0 * 20.0),
    ] {
        let runs = ink_runs(&page, 0, band.0, 300, band.1);
        assert_eq!(runs.len(), 2, "two glyphs in band {band:?}: {runs:?}");
        let advance = f64::from(runs[1].0) - f64::from(runs[0].0);
        assert!(
            (advance - expected).abs() <= 1.5,
            "band {band:?}: the second glyph starts {advance} px after the first, expected {expected}"
        );
    }
}

#[test]
fn a_type_0_font_without_a_program_is_reported_not_painted() {
    let page = render(7);
    // The page's font is a CIDFontType2 with no /FontFile2 and no ToUnicode:
    // there is nothing to draw the codes with, so the reason is reported.
    assert_eq!(
        page.report.unsupported.get("font:Type0-no-program"),
        Some(&1),
        "{:?}",
        page.report
    );
    assert_eq!(
        ink(&page, 0, 0, 300, 200),
        0,
        "nothing is painted for a font the renderer cannot draw"
    );
}

#[test]
fn an_embedded_truetype_program_is_drawn_from_its_own_outlines() {
    // Page 9 embeds a font this repository generates: `A` is a filled box from
    // (50, 0) to (550, 700) in a 1000-unit em, `B` the same box with a hole from
    // (200, 250) to (400, 450). At 40 pt on baseline y = 150 device y is 50, so
    // `A` covers x 22–42 and y 22–50 and `B`'s hole sits at x 52–60, y 32–40.
    let page = render(8);
    assert_eq!((page.canvas.width(), page.canvas.height()), (300, 200));
    expect_at(&page, 30, 35, BLACK, "A: the middle of the filled box");
    expect_at(&page, 48, 35, BLACK, "B: the left wall of the ring");
    expect_at(
        &page,
        55,
        35,
        WHITE,
        "B: the hole, which no fallback glyph has",
    );
    expect_at(&page, 75, 35, BLACK, "the second A");
    let runs = ink_runs(&page, 0, 22, 300, 50);
    assert_eq!(runs.len(), 3, "three glyphs: {runs:?}");
    assert_eq!(runs[0], (22, 42), "A spans 20 px at 40 pt");
    assert_eq!(runs[1], (46, 66), "B is the same box, one advance later");
    assert_eq!(runs[2], (70, 90), "the last A");
    // Two whole boxes and one with its 8×8 hole cut out.
    assert_eq!(
        ink(&page, 0, 0, 300, 200),
        2 * (20 * 28) + (20 * 28 - 8 * 8),
        "ink area"
    );
    assert_eq!(page.report.unsupported_total(), 0, "{}", page.report);
}

#[test]
fn composite_codes_map_to_cids_and_cids_to_glyph_ids() {
    // Page 10 is a Type 0 font: `/Encoding /Identity-H` means the hex string
    // <0001 0002 0001> is CIDs 1, 2, 1, and `/CIDToGIDMap /Identity` draws
    // glyphs 1, 2, 1 — the `A B A` of page 9, reached without a cmap.
    let page = render(9);
    assert_eq!((page.canvas.width(), page.canvas.height()), (300, 200));
    expect_at(&page, 30, 35, BLACK, "A: the filled box");
    expect_at(&page, 48, 35, BLACK, "B: the wall of the ring");
    expect_at(&page, 55, 35, WHITE, "B: the hole");
    expect_at(&page, 75, 35, BLACK, "the second A");
    let runs = ink_runs(&page, 0, 22, 300, 50);
    assert_eq!(runs, vec![(22, 42), (46, 66), (70, 90)], "{runs:?}");
    assert_eq!(
        ink(&page, 0, 0, 300, 200),
        2 * (20 * 28) + (20 * 28 - 8 * 8)
    );
    assert_eq!(page.report.unsupported_total(), 0, "{}", page.report);
}

#[test]
fn a_cid_to_gid_map_stream_decides_which_glyph_a_cid_draws() {
    // Page 11 is the same font with `/CIDToGIDMap` as a stream that maps
    // CID 1 -> glyph 2 and CID 2 -> glyph 1, so the same codes draw B A B. The
    // holes are what makes this unambiguous: no fallback glyph has one, and the
    // only way for them to move is for the map to have been read.
    let page = render(10);
    expect_at(&page, 24, 35, BLACK, "B first: the wall");
    expect_at(&page, 32, 35, WHITE, "B first: the hole");
    expect_at(&page, 50, 35, BLACK, "A in the middle");
    expect_at(&page, 74, 35, BLACK, "B last: the wall");
    expect_at(&page, 80, 35, WHITE, "B last: the hole");
    let runs = ink_runs(&page, 0, 22, 300, 50);
    assert_eq!(runs, vec![(22, 42), (46, 66), (70, 90)], "{runs:?}");
    assert_eq!(ink(&page, 0, 0, 300, 200), 2 * (20 * 28 - 8 * 8) + 20 * 28);
    assert_eq!(page.report.unsupported_total(), 0, "{}", page.report);
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
    for page in 0..11 {
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

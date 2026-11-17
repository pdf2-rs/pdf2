//! Tiling patterns (`/PatternType 1`, 8.7.3): four cells whose boundary is arithmetic.
//!
//! The fixture's cell is 10×10 and paints its left half black, so a 20×20 fill holds
//! exactly four cells and the black runs land at `x ∈ [0,5)` and `[10,15)`, twice
//! down the page. The assertions are those boundaries rather than "there is ink".

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn fixture() -> Vec<u8> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tiling.pdf");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn render(page: usize) -> RenderedPage {
    let document = Document::open(fixture()).expect("open fixture");
    render_page_number(&document, page, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("in bounds")
}

#[test]
fn four_cells_land_on_their_lattice() {
    let rendered = render(0);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the tile paints: {}",
        rendered.report
    );
    // The fill is 20×20 at the origin, so in the image it is the bottom-left corner:
    // rows 180..200 count from the top.
    for y in [185_u32, 195] {
        // First cell: black at 0..5, white at 5..10.
        assert_eq!(
            pixel(&rendered, 2, y),
            [0, 0, 0, 255],
            "cell 1 ink at y={y}"
        );
        assert_eq!(
            pixel(&rendered, 7, y),
            [255, 255, 255, 255],
            "cell 1 gap at y={y}"
        );
        // Second cell: the boundary is at x = 10, so black starts again there.
        assert_eq!(
            pixel(&rendered, 12, y),
            [0, 0, 0, 255],
            "cell 2 ink at y={y}"
        );
        assert_eq!(
            pixel(&rendered, 17, y),
            [255, 255, 255, 255],
            "cell 2 gap at y={y}"
        );
    }
    // Outside the fill nothing is painted — the tile is the fill's, not the page's.
    assert_eq!(
        pixel(&rendered, 25, 190),
        [255, 255, 255, 255],
        "outside the fill"
    );
}

#[test]
fn a_lattice_over_the_cell_budget_is_reported_not_attempted() {
    // Page 2 asks for `/XStep 0.01` over the same 20×20 fill: four million cells.
    let rendered = render(1);
    assert!(
        rendered.report.to_string().contains("pattern"),
        "the impossible lattice is reported: {}",
        rendered.report
    );
    assert!(
        rendered.canvas.is_blank(),
        "and nothing is painted, rather than a slice of a million cells"
    );
}

#[test]
fn an_uncoloured_cell_is_poured_with_the_colour_scn_gives_it() {
    // Page 3: `/PaintType 2`, so the cell is a stencil (`0 0 5 10 re f`, no colour),
    // and `1 0 0 /P3 scn` in `[/Pattern /DeviceRGB]` pours red through it (8.7.3.3).
    let rendered = render(2);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the stencil paints: {}",
        rendered.report
    );
    for y in [185_u32, 195] {
        assert_eq!(
            pixel(&rendered, 2, y),
            [255, 0, 0, 255],
            "poured red at y={y}"
        );
        assert_eq!(
            pixel(&rendered, 7, y),
            [255, 255, 255, 255],
            "stencil gap at y={y}"
        );
        assert_eq!(
            pixel(&rendered, 12, y),
            [255, 0, 0, 255],
            "second cell at y={y}"
        );
    }
}

#[test]
fn a_cell_that_sets_its_own_colour_is_reported_not_obeyed() {
    // Page 4 is the same stencil except that its cell runs `0 1 0 rg`, which the
    // clause forbids ("shall not explicitly specify any colours", 8.7.3.3). The
    // operator is reported and the poured colour stands, so the tile is red and
    // the page says why.
    let rendered = render(3);
    let report = rendered.report.to_string();
    assert!(
        report.contains("rg"),
        "the cell's colour operator is reported: {report}"
    );
    assert_eq!(
        pixel(&rendered, 2, 190),
        [255, 0, 0, 255],
        "the poured colour stands, not the cell's green"
    );
}

#[test]
fn tiling_type_one_snaps_its_cells_to_whole_device_pixels() {
    // `/TilingType` 1 and 3 ask for constant **device** spacing, and the only
    // lattice that has it is the pixel grid: a cell origin is rounded to a whole
    // device pixel, so spacing may drift by up to one pixel (8.7.3.1). Type 2 asks
    // for the opposite trade — no distortion, spacing may drift — and keeps the
    // exact pattern-space lattice.
    //
    // The fixture's cells are 2 units on a 2-unit lattice, each painting a
    // half-unit bar down its left edge, and it is rendered at 1.25 px/pt, so the
    // lattice is 2.5 device pixels. Page 1 (`/TilingType 1`) puts bar k at
    // 3k px — 0, 3, 5, 8 … — and page 2 (`/TilingType 2`) at 2.5k px — 0, 2.5, 5,
    // 7.5 …. Device pixel 2 is therefore white on page 1 and half covered on page 2,
    // which is the whole difference between the two types made visible.
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tiling-type.pdf");
    let document = Document::open(std::fs::read(&path).expect("fixture")).expect("open");
    let options = RenderOptions {
        scale: 1.25,
        ..RenderOptions::default()
    };

    let snapped = render_page_number(&document, 0, &options).expect("render page 1");
    assert_eq!(
        snapped.report.to_string(),
        "all operators supported",
        "the type-1 page paints: {}",
        snapped.report
    );
    // Bars on whole pixels: 0, 3, 5, 8, 10, 13 …
    for x in [0_u32, 3, 5, 8, 10, 13] {
        assert_ne!(
            pixel(&snapped, x, 4),
            [255, 255, 255, 255],
            "a bar starts at x = {x}"
        );
    }
    assert_eq!(
        pixel(&snapped, 2, 4),
        [255, 255, 255, 255],
        "no bar falls in device pixel 2 on the snapped lattice"
    );

    let exact = render_page_number(&document, 1, &options).expect("render page 2");
    assert_eq!(
        exact.report.to_string(),
        "all operators supported",
        "the type-2 page paints: {}",
        exact.report
    );
    // The unsnapped lattice is 2.5 px, so the second bar straddles pixels 2 and 3.
    assert_eq!(
        pixel(&exact, 2, 4),
        [128, 128, 128, 255],
        "the exact lattice half-covers device pixel 2"
    );
    assert_eq!(
        pixel(&exact, 3, 4),
        [223, 223, 223, 255],
        "and leaves the rest of the bar in pixel 3"
    );
}

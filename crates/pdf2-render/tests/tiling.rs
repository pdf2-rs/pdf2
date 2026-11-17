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

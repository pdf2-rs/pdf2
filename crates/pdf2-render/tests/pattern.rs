//! Tiling patterns on the corpus pages that use one.
//!
//! `colorkeymask.pdf` fills a 200×267 rectangle with a `/PatternType 1` cell, and
//! `ContentStreamCycleType3insideType3.pdf` selects one from inside a Type 3 char
//! procedure. Both were ignored in silence before the parser landed, then reported
//! while only the parser existed; now the lattice paints them, so both pages report
//! clean *and* carry the tile's ink — the report alone would be satisfied by a page
//! that paints nothing.

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

fn ink(rendered: &RenderedPage) -> usize {
    let mut painted = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if rendered.canvas.pixel(x, y) != Some([255, 255, 255, 255]) {
                painted += 1;
            }
        }
    }
    painted
}

#[test]
fn the_page_that_fills_with_a_tile_paints_it_through_its_key() {
    // `colorkeymask.pdf` is the joint case: one 200x267 `/PatternType 1` cell
    // (an image) filled through a `/Mask` colour key. The cell's red band is
    // masked, so the page's own background shows there while the green and blue
    // bands paint — the two features meeting on one page, neither of which worked
    // before this objective.
    let rendered = render("colorkeymask.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the tile paints: {}",
        rendered.report
    );
    let at = |x: u32, y: u32| rendered.canvas.pixel(x, y).expect("in bounds");
    // The fill is `18 823.89 200 -267 re`, i.e. screen x 18..218, y 18..285.
    assert_eq!(
        at(40, 100),
        [255, 255, 255, 255],
        "the keyed red band is the page"
    );
    assert_eq!(at(100, 100), [0, 255, 0, 255], "the green band paints");
    assert_eq!(at(170, 100), [0, 0, 255, 255], "the blue band paints");
    assert_eq!(
        at(5, 400),
        [255, 255, 255, 255],
        "and nothing leaks past the fill"
    );
}

#[test]
fn a_char_procedures_tile_paints_too() {
    // The pattern is selected inside a Type 3 glyph (`/Pattern cs /P1 scn`), which
    // the walker reaches through `run_type3_glyph`'s nested walk.
    let rendered = render("ContentStreamCycleType3insideType3.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the char procedure's tile paints: {}",
        rendered.report
    );
    assert!(ink(&rendered) > 5_000, "and the glyph has ink");
}

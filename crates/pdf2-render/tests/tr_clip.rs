//! The clipping half of `Tr` modes 4–7 (`9.3.6`, Table 104).
//!
//! Each fixture page paints a rectangle **after** the text, so the rectangle is
//! clipped to the glyph only when the mode asks for it. The glyph is the fallback
//! "A" at 96 pt, a 5×7 block letter with a counter, which is enough to tell a fill,
//! a clip and neither apart at known pixels:
//!
//! | Page | Mode | What it proves |
//! | --- | --- | --- |
//! | 1 | `7 Tr` | the clip exists and the glyph paints nothing: a blue "A" on white |
//! | 2 | `4 Tr` | the glyph fills *and* clips: blue left of `x = 50`, its own black beyond |
//! | 3 | `0 Tr` | the control: no clip, the rectangle covers the page |
//! | 4 | `7 Tr` in `q … Q` | the clip belongs to the state, so `Q` restores it |

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

fn fixture() -> Vec<u8> {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tr-clip.pdf");
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn render(page: usize) -> RenderedPage {
    let document = Document::open(fixture()).expect("open fixture");
    render_page_number(&document, page, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("in bounds")
}

fn blue_pixels(rendered: &RenderedPage) -> usize {
    let mut count = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if pixel(rendered, x, y) == BLUE {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn mode_seven_clips_to_the_glyph_and_paints_nothing_itself() {
    let rendered = render(0);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the clip costs no report: {}",
        rendered.report
    );
    // The glyph is ink; the page around it is not, though the rectangle covers the
    // whole page — that is the clip.
    assert_eq!(
        pixel(&rendered, 24, 16),
        BLUE,
        "the glyph's ink is the rectangle's colour"
    );
    assert_eq!(
        pixel(&rendered, 5, 5),
        WHITE,
        "outside the glyph the rectangle is clipped away"
    );
    assert_eq!(
        pixel(&rendered, 150, 100),
        WHITE,
        "and stays away across the page"
    );
    // The counter: white between two ink cells of the same row, so the clip follows
    // the outline's non-zero winding rather than its bounding box.
    assert_eq!(
        pixel(&rendered, 33, 16),
        WHITE,
        "the counter is a hole, not ink"
    );
    // A glyph's worth of pixels, not a page's.
    let blue = blue_pixels(&rendered);
    assert!(
        (1_000..8_000).contains(&blue),
        "the clip is the glyph, not the page: {blue} blue pixels"
    );
}

#[test]
fn mode_four_fills_the_glyph_and_clips_what_follows_it() {
    let rendered = render(1);
    // The rectangle covers `x < 50`, so the left of the glyph is blue and the right
    // is the glyph's own black fill: both halves of the mode are visible at once.
    assert_eq!(
        pixel(&rendered, 24, 40),
        BLUE,
        "left of the rectangle's edge"
    );
    assert_eq!(
        pixel(&rendered, 60, 40),
        BLACK,
        "beyond it, the glyph's fill stands"
    );
    assert_eq!(
        pixel(&rendered, 5, 5),
        WHITE,
        "and the clip still follows the outline"
    );
}

#[test]
fn mode_zero_paints_without_clipping() {
    let rendered = render(2);
    assert_eq!(
        pixel(&rendered, 5, 5),
        BLUE,
        "the control's rectangle is unclipped"
    );
    assert_eq!(pixel(&rendered, 199, 199), BLUE, "to the page's far corner");
    assert_eq!(
        blue_pixels(&rendered),
        200 * 200,
        "the whole page is the rectangle"
    );
}

#[test]
fn a_text_clip_ends_with_its_graphics_state() {
    let rendered = render(3);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the clip is restored, not reported: {}",
        rendered.report
    );
    assert_eq!(
        pixel(&rendered, 5, 5),
        BLUE,
        "after Q the rectangle is unclipped"
    );
    assert_eq!(blue_pixels(&rendered), 200 * 200, "so it covers the page");
}

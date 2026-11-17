//! `Tr` 4–7 with a Type 3 glyph: the clip has nothing to follow, and says so.
//!
//! A Type 3 glyph is a content stream, not an outline, so the clause's "as if they
//! were a path" has no path. The page must report `text-clip` and paint the rest
//! unclipped — the alternative, clipping to an empty mask, is a page that goes
//! blank with no explanation, which is the failure mode this renderer exists not
//! to have.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn render() -> RenderedPage {
    let path: PathBuf =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/tr-clip-type3.pdf");
    let document = Document::open(std::fs::read(&path).expect("fixture")).expect("open");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

#[test]
fn a_type3_glyph_under_a_clipping_mode_is_reported_by_name() {
    let rendered = render();
    let report = rendered.report.to_string();
    assert!(
        report.contains("text-clip"),
        "the clip a Type 3 glyph cannot give is named: {report}"
    );
    // And the page is not blank: the rectangle after the text is unclipped.
    for (x, y) in [(5_u32, 5_u32), (100, 100), (199, 199)] {
        assert_eq!(
            rendered.canvas.pixel(x, y),
            Some([0, 0, 255, 255]),
            "the rectangle covers ({x}, {y})"
        );
    }
}

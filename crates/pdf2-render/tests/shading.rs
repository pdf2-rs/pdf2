//! Shadings (`8.7.4`): axial, radial and function-based gradients.
//!
//! `tests/fixtures/shading.pdf` is arithmetic, not taste: an axial gradient from
//! red at x = 0 to blue at x = 200, and a radial one from yellow at the centre to
//! black at radius 80 with `/Extend [false false]`, so outside the circle nothing
//! is painted at all.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn render(path: &str, page: usize) -> RenderedPage {
    let bytes = std::fs::read(repo(path)).expect("fixture");
    let document = Document::open(bytes).expect("open");
    render_page_number(&document, page, &RenderOptions::default()).expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("inside")
}

fn close(actual: [u8; 4], expected: [u8; 3]) -> bool {
    (0..3).all(|channel| (i32::from(actual[channel]) - i32::from(expected[channel])).abs() <= 3)
}

#[test]
fn an_axial_shading_follows_its_function() {
    let rendered = render("tests/fixtures/shading.pdf", 0);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the shading is painted, not reported: {}",
        rendered.report
    );
    // The expectation is the formula, not a magic number: at device x the axis
    // parameter is (x + 0.5) / 200 between red (255, 0, 0) and blue (0, 0, 255).
    for x in [12u32, 100, 188] {
        #[allow(clippy::cast_precision_loss)]
        let t = (f64::from(x) + 0.5) / 200.0;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let expected = [
            ((1.0 - t) * 255.0).round() as u8,
            0,
            (t * 255.0).round() as u8,
        ];
        assert!(
            close(pixel(&rendered, x, 100), expected),
            "x {x}: got {:?}, expected {expected:?}",
            pixel(&rendered, x, 100)
        );
    }
    assert!(
        close(pixel(&rendered, 100, 100), [128, 0, 128]),
        "half red, half blue at the middle: {:?}",
        pixel(&rendered, 100, 100)
    );
    assert!(
        close(pixel(&rendered, 188, 100), [16, 0, 239]),
        "blue near the end: {:?}",
        pixel(&rendered, 188, 100)
    );
}

#[test]
fn a_radial_shading_is_bounded_by_extend() {
    let rendered = render("tests/fixtures/shading.pdf", 1);
    assert!(
        close(pixel(&rendered, 100, 100), [255, 255, 0]),
        "yellow centre"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let rim = {
        // y = 24.5 is 75.5 from the centre of an 80-pixel radius: t = 0.944, so
        // yellow (255, 255, 0) has faded by that fraction.
        let t: f64 = (100.0 - 24.5) / 80.0;
        let value = ((1.0 - t) * 255.0).round() as u8;
        [value, value, 0]
    };
    assert!(
        close(pixel(&rendered, 100, 24), rim),
        "near black at the rim: got {:?}, expected {rim:?}",
        pixel(&rendered, 100, 24)
    );
    assert_eq!(
        pixel(&rendered, 5, 5),
        [255, 255, 255, 255],
        "`/Extend [false false]` leaves the corners untouched"
    );
}

#[test]
fn the_corpus_function_shading_page_stops_reporting_sh() {
    // `function_based_shading.pdf` draws nine type-1 shadings through type-4
    // calculators; it used to report `sh` nine times.
    let rendered = render("corpus/tier-a/function_based_shading.pdf", 0);
    assert!(
        !rendered.report.to_string().contains("sh"),
        "every shading is painted: {}",
        rendered.report
    );
    let mut ink = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if rendered.canvas.pixel(x, y) != Some([255, 255, 255, 255]) {
                ink += 1;
            }
        }
    }
    assert!(ink > 50_000, "the page is covered in gradients: {ink}");
}

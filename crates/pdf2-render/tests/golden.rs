//! Golden-image and pixel-level tests for the renderer.
//!
//! `tests/fixtures/paths.pdf` is drawn by the committed generator: a red fill,
//! a blue even-odd pair with a hole, a green stroke, a clipped fill, a
//! transformed square, a dashed line — and one line of text, which is reported
//! as unsupported until glyph outlines land. The golden PNG pins every pixel;
//! the assertions below pin what those pixels mean.

use std::path::{Path, PathBuf};
use std::process::Command;

use pdf2::Document;
use pdf2_render::{encode_png, render_page_number, RenderOptions, RenderedPage};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(repo(&format!("tests/fixtures/{name}"))).expect("read fixture")
}

fn render(relative: &str, page: usize, scale: f64) -> RenderedPage {
    let document = Document::open(fixture(relative)).expect("open fixture");
    render_page_number(
        &document,
        page,
        &RenderOptions {
            scale,
            ..RenderOptions::default()
        },
    )
    .expect("render")
}

fn pixel(rendered: &RenderedPage, x: u32, y: u32) -> [u8; 4] {
    rendered.canvas.pixel(x, y).expect("inside the canvas")
}

const WHITE: [u8; 4] = [255, 255, 255, 255];
const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const GREEN: [u8; 4] = [0, 255, 0, 255];
const CYAN: [u8; 4] = [0, 255, 255, 255];
const MAGENTA: [u8; 4] = [255, 0, 255, 255];
const BLACK: [u8; 4] = [0, 0, 0, 255];

#[test]
fn fills_strokes_clips_transforms_and_dashes_land_where_they_should() {
    let rendered = render("paths.pdf", 0, 1.0);
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (200, 200)
    );

    // Red fill: user space (20,20)–(80,80), device y is flipped.
    assert_eq!(pixel(&rendered, 30, 150), RED);
    // Still background just outside it.
    assert_eq!(pixel(&rendered, 15, 150), WHITE);

    // Even-odd pair: the overlap is a hole, the rest is blue.
    assert_eq!(pixel(&rendered, 110, 170), BLUE);
    assert_eq!(pixel(&rendered, 130, 140), WHITE);
    // Non-overlapping part of the second rectangle is blue as well.
    assert_eq!(pixel(&rendered, 170, 130), BLUE);

    // Green stroke, 8 wide: the band is painted, the rectangle's inside is not.
    assert_eq!(pixel(&rendered, 21, 60), GREEN);
    assert_eq!(pixel(&rendered, 50, 65), WHITE);

    // Clip: the cyan fill only exists inside the 40×40 clip rectangle. The
    // probe for "outside" avoids the dashed line's row.
    assert_eq!(pixel(&rendered, 30, 40), CYAN);
    assert_eq!(pixel(&rendered, 100, 30), WHITE);

    // Transform: translate(140,120) then scale 0.5, rectangle (0,0)-(20,20).
    assert_eq!(pixel(&rendered, 145, 75), BLACK);
    assert_eq!(pixel(&rendered, 155, 75), WHITE);

    // Dashes [6 4] starting at x=90: on at 92, off at 98.
    assert_eq!(pixel(&rendered, 92, 40), MAGENTA);
    assert_eq!(pixel(&rendered, 98, 40), WHITE);

    // The text line is not drawn yet, and everything else is background.
    assert_eq!(pixel(&rendered, 25, 10), WHITE);
    assert_eq!(pixel(&rendered, 5, 5), WHITE);
}

#[test]
fn unsupported_operators_are_reported_not_fatal() {
    let rendered = render("paths.pdf", 0, 1.0);
    assert!(
        rendered.report.unsupported_total() >= 4,
        "{:?}",
        rendered.report
    );
    assert_eq!(rendered.report.unsupported.get("Tj"), Some(&1));
    assert_eq!(rendered.report.unsupported.get("Tf"), Some(&1));
    assert!(
        rendered.report.to_string().contains("Tj"),
        "{}",
        rendered.report
    );
    // The paths still painted: the red fill is there.
    assert_eq!(pixel(&rendered, 30, 150), RED);
}

#[test]
fn scaling_multiplies_the_canvas_and_the_geometry() {
    let rendered = render("paths.pdf", 0, 2.0);
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (400, 400)
    );
    // The red fill now covers device (40,40)–(160,160), so (60,300) is red.
    assert_eq!(pixel(&rendered, 60, 300), RED);
}

#[test]
fn a_page_without_content_is_uniformly_background() {
    // page-tree.pdf page 2 has no /Contents at all.
    let rendered = render("page-tree.pdf", 1, 1.0);
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (612, 792)
    );
    for (x, y) in [(0, 0), (611, 0), (300, 400), (611, 791)] {
        assert_eq!(pixel(&rendered, x, y), WHITE, "({x}, {y})");
    }
    assert_eq!(rendered.report.unsupported_total(), 0);
}

#[test]
fn the_committed_golden_matches_the_renderer() {
    let rendered = render("paths.pdf", 0, 1.0);
    let png = encode_png(&rendered.canvas);
    let golden_path = repo("tests/fixtures/golden/paths.png");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&golden_path, &png).expect("bless golden");
        return;
    }
    let golden = std::fs::read(&golden_path).expect("read golden");
    assert_eq!(
        golden, png,
        "the golden image differs; re-run the golden binary with --bless if the change is intended"
    );
}

#[test]
fn the_golden_cli_blesses_and_verifies() {
    let mut out = std::env::temp_dir();
    out.push(format!("pdf2-render-golden-{}.png", std::process::id()));
    let out_path = out.to_str().expect("utf-8").to_owned();
    let binary = env!("CARGO_BIN_EXE_pdf2-render-golden");

    let blessed = Command::new(binary)
        .args(["--bless", "--out", &out_path])
        .output()
        .expect("run golden binary");
    assert!(blessed.status.success(), "{:?}", blessed);

    let verified = Command::new(binary)
        .args(["--out", &out_path])
        .output()
        .expect("run");
    assert!(verified.status.success(), "{:?}", verified);

    std::fs::write(&out, b"not a png").expect("corrupt");
    let failed = Command::new(binary)
        .args(["--out", &out_path])
        .output()
        .expect("run");
    assert_eq!(failed.status.code(), Some(1), "a mismatch is exit 1");
    let _ = std::fs::remove_file(&out);
}

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
use pdf2_render::{RenderOptions, RenderedPage, encode_png, render_page_number};

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(repo(&format!("tests/fixtures/{name}"))).expect("read fixture")
}

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

    // The text line renders above the baseline at y = 10 device pixels.
    assert!(ink(&rendered, 15, 0, 150, 12) > 50, "text ink");
    assert_eq!(pixel(&rendered, 5, 5), WHITE);
}

#[test]
fn the_fixtures_use_only_supported_operators() {
    for name in ["paths.pdf", "images.pdf"] {
        let rendered = render(name, 0, 1.0);
        assert_eq!(
            rendered.report.unsupported_total(),
            0,
            "{name}: {}",
            rendered.report
        );
    }
}

#[test]
fn unknown_operators_are_still_counted_not_fatal() {
    let mut builder = pdf2::Document::new();
    builder.add_page(
        pdf2::Rect::new(0.0, 0.0, 50.0, 50.0),
        "q 1 0 0 rg 5 5 20 20 re f /GS1 gs Q",
    );
    let bytes = builder.save().expect("save");
    let document = Document::open(bytes).expect("open");
    let rendered = render_page_number(&document, 0, &RenderOptions::default()).expect("render");
    assert_eq!(
        rendered.report.unsupported.get("gs"),
        Some(&1),
        "{:?}",
        rendered.report
    );
    // The fill before it still painted.
    assert_eq!(pixel(&rendered, 10, 40), RED);
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
    // page-tree.pdf page 2 has no /Contents at all, and it inherits /Rotate 90
    // from its parent, so the canvas is the media box turned a quarter.
    let rendered = render("page-tree.pdf", 1, 1.0);
    // Its `/CropBox [10 20 190 280]` is the visible box now — 180x260, turned a
    // quarter by the inherited `/Rotate 90`.
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (260, 180)
    );
    for (x, y) in [(0, 0), (259, 0), (130, 90), (259, 179)] {
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
fn a_book_page_is_set_in_its_embedded_serif_font() {
    // book.pdf embeds a Crimson Text subset (OFL, committed with its licence)
    // and sets six paragraphs of public-domain prose with the font's own
    // advances: 420x595 pt, 54 pt margins, 11.5 pt on 16 pt leading.
    let rendered = render("book.pdf", 0, 1.0);
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (420, 595)
    );
    assert_eq!(
        rendered.report.unsupported_total(),
        0,
        "{}",
        rendered.report
    );

    let ink = ink(&rendered, 0, 0, 420, 595);
    assert!(
        ink > 6_000,
        "a page of prose should cover thousands of pixels, got {ink}"
    );
    // The first line starts at the left margin and nowhere else does ink start
    // outside it: the measure and the margins are what the generator typeset to.
    let left_edge = (0..420)
        .find(|&x| (0..595).any(|y| pixel(&rendered, x, y) != WHITE))
        .expect("some ink");
    assert!(
        (54..=58).contains(&left_edge),
        "the text block starts at x {left_edge}, expected the 54 pt margin"
    );

    // The paragraphs are extractable as text, which means the /Widths and the
    // encoding agree with what was drawn.
    let document = Document::open(fixture("book.pdf")).expect("open book.pdf");
    let page = &document.pages().expect("pages")[0];
    let text = document.extract_text(page).expect("extract");
    assert!(
        text.contains("The Time Traveller") && text.contains("recondite matter"),
        "extracted text: {:?}",
        &text[..text.len().min(120)]
    );

    let png = encode_png(&rendered.canvas);
    let golden_path = repo("tests/fixtures/golden/book.png");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&golden_path, &png).expect("bless golden");
        return;
    }
    let golden = std::fs::read(&golden_path).expect("read golden");
    assert!(
        golden == png,
        "book.png differs; re-run with BLESS=1 if the change is intended"
    );
}

#[test]
fn the_crop_box_and_user_unit_define_the_visible_page() {
    // MediaBox [0 0 300 200], CropBox [50 60 250 260], UserUnit 2: the raster is
    // the crop box at twice its size (400x400), with the crop box at the origin,
    // and content past its right edge is cut off by the canvas.
    let rendered = render("cropbox.pdf", 0, 1.0);
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (400, 400)
    );
    assert_eq!(
        rendered.report.unsupported_total(),
        0,
        "{}",
        rendered.report
    );
    // Red: user (60,70)-(90,100) -> device (20,320)-(80,380).
    assert_eq!(pixel(&rendered, 50, 350), RED);
    // Blue: user (200,220)-(230,250) -> device (300,20)-(360,60).
    assert_eq!(pixel(&rendered, 330, 40), BLUE);
    // Green: user (230,60)-(270,100) crosses the crop's right edge at x = 250,
    // so it reaches the canvas edge and stops there.
    assert_eq!(pixel(&rendered, 390, 390), [0, 153, 0, 255], "0.6 green");
    assert_eq!(pixel(&rendered, 399, 399), [0, 153, 0, 255]);
    assert_eq!(
        pixel(&rendered, 200, 200),
        WHITE,
        "inside the crop, unpainted"
    );
    // Two whole 60x60 rectangles and one clipped to 40x80 at the crop edge.
    assert_eq!(
        ink(&rendered, 0, 0, 400, 400),
        3_600 + 3_600 + 3_200,
        "ink area"
    );

    let png = encode_png(&rendered.canvas);
    let golden_path = repo("tests/fixtures/golden/cropbox.png");
    if std::env::var_os("BLESS").is_some() {
        std::fs::write(&golden_path, &png).expect("bless golden");
        return;
    }
    let golden = std::fs::read(&golden_path).expect("read golden");
    assert!(
        golden == png,
        "cropbox.png differs; re-run with BLESS=1 if the change is intended"
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

#[test]
fn an_invisible_text_mode_paints_nothing() {
    // `text-mode.pdf` draws the same size of text twice: `Tr 0` at y = 150 and
    // `Tr 3` at y = 100. Only the first band may hold any ink.
    let rendered = render("text-mode.pdf", 0, 1.0);
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "Tr is a supported operator"
    );
    let painted = ink(&rendered, 0, 20, 200, 60);
    let hidden = ink(&rendered, 0, 80, 200, 120);
    assert!(painted > 20, "the visible line is drawn: {painted}");
    assert_eq!(hidden, 0, "Tr 3 draws no ink at all");
}

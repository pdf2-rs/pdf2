//! `/JBIG2Decode` (7.4.7): bi-level scans, behind the `jbig2` feature.
//!
//! The two pdf.js pages are the same 399×400 crop of scanned text, encoded twice:
//! `bitmap-composite-and-xnor.pdf` carries its segments inline, and
//! `bitmap-p32-eof.pdf` keeps them in a `/JBIG2Globals` stream — which is the
//! clause's separate-segments case and the reason a page wired only to
//! `Page::new_embedded(data, None, …)` fails on it.
//!
//! What is proven here: both pages paint, the globals page is not blank, the two
//! decode to the **same pixels** (so the shared dictionaries are really applied),
//! and the sense is ink on white rather than its negative — `hayro-jbig2` reports
//! `black` while a 1-bit PDF sample is 1 for white.

#![cfg(feature = "jbig2")]

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{RenderOptions, RenderedPage, render_page_number};

fn corpus(name: &str) -> Vec<u8> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/tier-a")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

fn render(name: &str) -> RenderedPage {
    let document = Document::open(corpus(name)).expect("open");
    render_page_number(&document, 0, &RenderOptions::default()).expect("render")
}

/// Black and white counts for a rendered page.
fn ink(rendered: &RenderedPage) -> (usize, usize) {
    let mut black = 0;
    let mut white = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            match rendered.canvas.pixel(x, y) {
                Some([0, 0, 0, 255]) => black += 1,
                Some([255, 255, 255, 255]) => white += 1,
                Some(other) => panic!("a bi-level page painted {other:?}"),
                None => panic!("({x}, {y}) is outside the canvas"),
            }
        }
    }
    (black, white)
}

#[test]
fn the_inline_page_paints_ink_on_white() {
    let rendered = render("bitmap-composite-and-xnor.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the image draws instead of being reported: {}",
        rendered.report
    );
    let (black, white) = ink(&rendered);
    assert_eq!(black + white, 399 * 400, "every pixel is one of the two");
    assert!(black > 5_000, "the page has ink: {black} black pixels");
    assert!(
        white > black * 4,
        "the sense is ink on white, not the negative: {black} black against {white} white"
    );
}

#[test]
fn the_globals_page_paints_the_same_pixels() {
    let inline = render("bitmap-composite-and-xnor.pdf");
    let globals = render("bitmap-p32-eof.pdf");
    assert_eq!(
        globals.report.to_string(),
        "all operators supported",
        "the globals page draws: {}",
        globals.report
    );
    let (black, white) = ink(&globals);
    assert!(black > 5_000, "the globals page is not blank: {black}");
    assert!(white > black * 4, "and it is ink on white too");
    // The pair is one image encoded two ways, so the shared dictionaries must
    // produce the same raster: a page that ignored `/JBIG2Globals` could still
    // paint something, but not this.
    for y in 0..inline.canvas.height() {
        for x in 0..inline.canvas.width() {
            assert_eq!(
                inline.canvas.pixel(x, y),
                globals.canvas.pixel(x, y),
                "the two encodings disagree at ({x}, {y})"
            );
        }
    }
}

/// A `P4` (binary) PBM, as `corpus/reference/` stores the reference rasters.
///
/// The committed form is PBM rather than the upstream PNG because this crate has
/// no PNG *decoder* — `src/png.rs` only writes — and the rasters are 1-bit, so P4
/// loses nothing. Bit `1` is black, which is the PDF sample value 0.
struct Pbm {
    width: u32,
    height: u32,
    black: Vec<bool>,
}

impl Pbm {
    fn parse(bytes: &[u8]) -> Self {
        let mut fields = Vec::new();
        let mut at = 0;
        // `P4`, then width, then height, each separated by whitespace.
        while fields.len() < 3 {
            while at < bytes.len() && bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            let start = at;
            while at < bytes.len() && !bytes[at].is_ascii_whitespace() {
                at += 1;
            }
            fields.push(String::from_utf8_lossy(&bytes[start..at]).into_owned());
        }
        assert_eq!(fields[0], "P4", "the reference is a binary PBM");
        let width: u32 = fields[1].parse().expect("PBM width");
        let height: u32 = fields[2].parse().expect("PBM height");
        at += 1; // the single whitespace byte after the height
        let stride = (width as usize).div_ceil(8);
        assert_eq!(
            bytes.len() - at,
            stride * height as usize,
            "the PBM body is width × height bits"
        );
        let mut black = Vec::with_capacity(width as usize * height as usize);
        for row in 0..height as usize {
            for column in 0..width as usize {
                let byte = bytes[at + row * stride + column / 8];
                black.push((byte >> (7 - column % 8)) & 1 == 1);
            }
        }
        Self {
            width,
            height,
            black,
        }
    }

    fn at(&self, x: u32, y: u32) -> bool {
        self.black[y as usize * self.width as usize + x as usize]
    }
}

fn reference() -> Pbm {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/reference/bitmap-composite-and-xnor.pbm");
    let bytes = std::fs::read(&path).unwrap_or_else(|error| {
        panic!(
            "{}: {error} — the reference rasters live in corpus/reference/",
            path.display()
        )
    });
    Pbm::parse(&bytes)
}

#[test]
fn the_reference_raster_records_the_expected_difference() {
    // pdfium's own expected raster for this page, committed under `corpus/reference/`
    // (see the README there). It is the independent arbiter the oracle case list has
    // been missing: our decoder and the three engines disagree, and this says which
    // raster is the one a second implementation produced.
    //
    // The numbers are **pinned as they are**, not as they should be: the gap is open
    // work, and a test that tolerated it would not notice the day it changes. Any
    // movement — a fix, or a regression — fails here and has to be explained.
    let reference = reference();
    assert_eq!((reference.width, reference.height), (399, 400));
    let dark = reference.black.iter().filter(|black| **black).count();
    assert_eq!(dark, 10_950, "the reference raster is intact");

    let rendered = render("bitmap-composite-and-xnor.pdf");
    assert_eq!(
        (rendered.canvas.width(), rendered.canvas.height()),
        (reference.width, reference.height),
        "the page and the reference are the same size"
    );

    let mut flipped = 0_u64;
    let mut columns = vec![0_u64; reference.width as usize];
    for y in 0..reference.height {
        for x in 0..reference.width {
            let Some(pixel) = rendered.canvas.pixel(x, y) else {
                panic!("({x}, {y}) is outside the canvas");
            };
            let ours_black = pixel[..3] == [0, 0, 0];
            if ours_black != reference.at(x, y) {
                flipped += 1;
                columns[x as usize] += 1;
            }
        }
    }
    let pixels = u64::from(reference.width) * u64::from(reference.height);
    let mean = 255.0 * flipped as f64 / pixels as f64;
    assert_eq!(
        flipped, 19_553,
        "pixels that differ from the reference (mean |Δ| {mean:.2} of 255)"
    );
    assert!(
        (mean - 31.24).abs() < 0.01,
        "mean |Δ| against the reference: {mean:.2}"
    );
    // The disagreement is a misplacement, not missing ink: ~9.6 k stroke pixels sit
    // on the wrong side of a pixel boundary, and they cluster in one column band.
    let (busiest, count) = columns
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)
        .map(|(column, count)| (column as u32, *count))
        .expect("a column");
    assert!(
        (320..=340).contains(&busiest),
        "the busiest column is x = {busiest} with {count} flips"
    );
    assert!(count >= 120, "x = {busiest} carries {count} flips");
}

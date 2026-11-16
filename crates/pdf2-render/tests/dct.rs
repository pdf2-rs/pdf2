//! `/DCTDecode`: the JPEG decoder, behind the `dct` feature.
//!
//! What is proven here: a four-component (CMYK) JPEG is un-inverted as PDF 7.4.8
//! requires and converted with the multiplicative rule, quadrants exact; and the
//! corpus page whose image is CMYK draws its photograph instead of being reported.

use std::path::{Path, PathBuf};

use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions, RenderedPage};

fn corpus(name: &str) -> Option<Vec<u8>> {
    let path: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/tier-a")
        .join(name);
    std::fs::read(&path).ok()
}

fn render(bytes: Vec<u8>, options: &RenderOptions) -> RenderedPage {
    let document = Document::open(bytes).expect("open");
    render_page_number(&document, 0, options).expect("render")
}

#[cfg(feature = "dct")]
#[test]
fn a_cmyk_jpeg_is_uninverted_and_converted_per_ink() {
    // The fixture's JPEG carries an Adobe APP14 marker, so its samples are stored
    // inverted; the four quadrants are white, cyan, red and black in ink. Skipping
    // the inversion draws the negative — which is exactly what this catches.
    let rendered = render(
        std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/cmyk-jpeg.pdf"),
        )
        .expect("fixture"),
        &RenderOptions::default(),
    );
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "the CMYK image draws: {}",
        rendered.report
    );
    let pixel = |x: u32, y: u32| rendered.canvas.pixel(x, y).expect("in bounds");
    let near = |got: [u8; 4], want: [u8; 3]| {
        let off = (0..3)
            .map(|i| i32::from(got[i]) - i32::from(want[i]))
            .map(i32::abs)
            .max()
            .unwrap_or(0);
        assert!(off <= 8, "got {got:?}, want {want:?} (±8)");
    };
    near(pixel(8, 8), [255, 255, 255]); // no ink
    near(pixel(24, 8), [0, 255, 255]); // cyan
    near(pixel(8, 24), [255, 0, 0]); // magenta and yellow
    near(pixel(24, 24), [0, 0, 0]); // full black
}

#[cfg(feature = "dct")]
#[test]
fn the_corpus_cmyk_photograph_draws() {
    let Some(bytes) = corpus("cmykjpeg.pdf") else {
        return;
    };
    let rendered = render(bytes, &RenderOptions::default());
    assert!(
        !rendered.report.to_string().contains("CMYK"),
        "the CMYK image is no longer refused: {}",
        rendered.report
    );
    assert!(
        !rendered.report.to_string().contains("Do"),
        "and it is drawn rather than reported: {}",
        rendered.report
    );
    // The photograph is an iceberg: bright ice against dark water, so the page has
    // both very light and very dark pixels.
    let mut light = 0;
    let mut dark = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if let Some([r, g, b, _]) = rendered.canvas.pixel(x, y) {
                let luma = (u32::from(r) + u32::from(g) + u32::from(b)) / 3;
                if luma > 150 {
                    light += 1;
                } else if luma < 40 {
                    dark += 1;
                }
            }
        }
    }
    assert!(light > 200, "the ice is bright: {light} light pixels");
    assert!(dark > 200, "the water is dark: {dark} dark pixels");
}

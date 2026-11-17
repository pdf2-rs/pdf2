//! CCITT fax images: one-dimensional rows, G3 mixed-mode lines, and G4.
//!
//! The corpus page is the proof: its six images are labelled with the mode they
//! use (`/K -1`, `/K 0`, `/K 1`, two of each, with both `/EndOfBlock` values), so
//! a mode that decodes wrongly is visible as a wrong label rather than as a blank
//! page. Before two-dimensional decoding landed the page reported `Do×4`: only the
//! `/K 0` images drew.

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

#[test]
fn every_fax_mode_on_the_corpus_page_decodes() {
    let rendered = render("ccitt_EndOfBlock_false.pdf");
    assert_eq!(
        rendered.report.to_string(),
        "all operators supported",
        "all six images draw, in all three modes: {}",
        rendered.report
    );
}

#[test]
fn the_two_dimensional_images_have_text_in_them() {
    // Each label is drawn on a white box: the page must have both the black glyphs
    // and the white boxes, and it must have the blue backdrop they sit on. A
    // decoder that produced plausible-but-wrong lines would still have ink, which
    // is why the fixtures below pin the *shape* rather than the ink count.
    let rendered = render("ccitt_EndOfBlock_false.pdf");
    let mut black = 0;
    let mut white = 0;
    let mut blue = 0;
    for y in 0..rendered.canvas.height() {
        for x in 0..rendered.canvas.width() {
            if let Some([r, g, b, _]) = rendered.canvas.pixel(x, y) {
                let (r, g, b) = (u32::from(r), u32::from(g), u32::from(b));
                if r < 60 && g < 60 && b < 60 {
                    black += 1;
                } else if r > 240 && g > 240 && b > 240 {
                    white += 1;
                } else if b > 200 && b > r + 60 {
                    blue += 1;
                }
            }
        }
    }
    assert!(black > 2_000, "the glyphs are drawn: {black} black pixels");
    assert!(white > 20_000, "the boxes are white: {white} white pixels");
    assert!(blue > 50_000, "the backdrop is blue: {blue} blue pixels");
}

#[test]
fn the_corpus_g4_image_decodes_to_a_picture() {
    // The `/K -1` image decoded on its own: 81 columns by 26 rows. A two-dimensional
    // decode that produced noise would still be 286 bytes, so the claim is that the
    // rows *differ* — a box with glyphs, not a single fill — and that the border
    // rows are uniform, which is what a box looks like.
    let document = Document::open(corpus("ccitt_EndOfBlock_false.pdf")).expect("open");
    for number in 1..400_u32 {
        let Ok(pdf2::Object::Stream(stream)) = document.get_object_by_number(number) else {
            continue;
        };
        let Some(pdf2::Object::Dictionary(parms)) = stream.dict().get(b"DecodeParms") else {
            continue;
        };
        if parms.get(b"K").and_then(pdf2::Object::as_integer) != Some(-1) {
            continue;
        }
        let bytes = document.decode_stream(&stream).expect("G4 decodes");
        assert_eq!(bytes.len(), 26 * 11, "26 rows of 81 columns");
        let rows: Vec<&[u8]> = bytes.chunks(11).collect();
        assert!(
            rows[0].iter().all(|byte| *byte == rows[0][0]),
            "the first row is a uniform border"
        );
        assert!(
            rows.iter().any(|row| row != &rows[0]),
            "and the glyph rows differ from it"
        );
        assert!(
            rows[13].iter().any(|byte| *byte != rows[13][0]),
            "a middle row carries glyph ink"
        );
        return;
    }
    panic!("the page has no /K -1 image");
}

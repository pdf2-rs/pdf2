//! `/CCITTFaxDecode` (7.4.6): the one-dimensional half.
//!
//! `ccitt_EndOfBlock_false.pdf` carries six fax images spanning every `/K` value
//! with both `/EndOfBlock` values, so one page exercises the whole parameter set.
//! The two `/K 0` images decode from the first commit of the decoder; the four
//! two-dimensional ones are reported, because a half-decoded fax line is noise
//! rather than an approximation.

use std::path::{Path, PathBuf};

use pdf2::Document;

fn repo(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

#[test]
fn the_one_dimensional_images_decode_and_the_two_dimensional_ones_are_reported() {
    let bytes = std::fs::read(repo("corpus/tier-a/ccitt_EndOfBlock_false.pdf")).expect("corpus");
    let document = Document::open(bytes).expect("open");
    let page = document.page(0).expect("page");
    let rendered =
        pdf2_render::render_page_number(&document, 0, &pdf2_render::RenderOptions::default())
            .expect("render");
    // Two `/K 0` images drew, four `/K 1` and `/K -1` ones were reported.
    assert_eq!(
        rendered.report.to_string(),
        "Do×4",
        "only the two-dimensional images are missing: {}",
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
    assert!(ink > 5_000, "the fax images put ink on the page: {ink}");
    let _ = page;
}

#[test]
fn a_fax_stream_decodes_to_rows_of_the_declared_width() {
    // The first `/CCITTFaxDecode` stream in the file, decoded through the public
    // filter API: 81 columns is eleven bytes per row, and the file declares 26.
    let bytes = std::fs::read(repo("corpus/tier-a/ccitt_EndOfBlock_false.pdf")).expect("corpus");
    let document = Document::open(bytes).expect("open");
    let mut found = false;
    for number in 1..=document
        .trailer()
        .get(b"Size")
        .and_then(pdf2::Object::as_integer)
        .unwrap_or(32)
    {
        let number = u32::try_from(number).unwrap_or(0);
        if !document.contains_object(number) {
            continue;
        }
        let Ok(pdf2::Object::Stream(stream)) = document.get_object_by_number(number) else {
            continue;
        };
        let is_fax = matches!(stream.dict().get(b"Filter"), Some(pdf2::Object::Name(name)) if name == b"CCITTFaxDecode");
        if !is_fax {
            continue;
        }
        let Ok(decoded) = document.decode_stream(&stream) else {
            continue;
        };
        // 81 columns → 11 bytes per row (ceil(81/8)), 26 rows.
        assert_eq!(decoded.len() % 11, 0, "rows are eleven bytes wide");
        assert_eq!(decoded.len(), 11 * 26, "twenty-six rows");
        found = true;
        break;
    }
    assert!(found, "the corpus file has a fax stream");
}

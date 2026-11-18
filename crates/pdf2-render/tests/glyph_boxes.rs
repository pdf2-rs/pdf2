//! The painted glyph boxes (`GlyphBox`, `render_page_number_traced`).
//!
//! The text layer over the demo's canvas is built from these, so the numbers
//! have to be the numbers the renderer painted: the fallback alphabet advances
//! every glyph by 6/9 em, and a font that states `/Widths` advances by those.

use pdf2::{Document, Object};
use pdf2_render::{RenderOptions, render_page_number, render_page_number_traced};

const TEXT: &[u8] = include_bytes!("../../../tests/fixtures/text.pdf");
const BOOK: &[u8] = include_bytes!("../../../tests/fixtures/book.pdf");

fn traced(bytes: &[u8]) -> pdf2_render::TracedPage {
    let document = Document::open(bytes).expect("the fixture opens");
    render_page_number_traced(&document, 0, &RenderOptions::default()).expect("the page renders")
}

#[test]
fn the_fallback_alphabet_advances_every_glyph_by_six_ninths_of_an_em() {
    let page = traced(TEXT);
    let first_line: Vec<&pdf2_render::GlyphBox> = page
        .glyphs
        .iter()
        .take_while(|glyph| glyph.text != "\n" && glyph.y == page.glyphs[0].y)
        .collect();
    let text: String = first_line.iter().map(|glyph| glyph.text.as_str()).collect();
    assert_eq!(text, "Hello, PDF 2.0", "the first line's characters");

    let size = first_line[0].size;
    assert!(
        (size - 18.0).abs() < 1e-9,
        "the fixture sets 18 pt type, got {size}"
    );
    for glyph in &first_line {
        assert!(
            (glyph.advance - 6.0 / 9.0 * size).abs() < 1e-9,
            "{:?} advances {}, expected 6/9 em = {}",
            glyph.text,
            glyph.advance,
            6.0 / 9.0 * size
        );
    }
    // And they really are evenly spaced along the line.
    for pair in first_line.windows(2) {
        let step = pair[1].x - pair[0].x;
        assert!(
            (step - 6.0 / 9.0 * size).abs() < 1e-9,
            "the pen moved {step} between {:?} and {:?}",
            pair[0].text,
            pair[1].text
        );
    }
}

#[test]
fn a_line_is_a_run_of_boxes_and_a_space_is_one_of_them() {
    let page = traced(TEXT);
    let spaces = page.glyphs.iter().filter(|glyph| glyph.text == " ").count();
    assert!(spaces >= 2, "a space is a glyph with an advance: {spaces}");
    let space = page
        .glyphs
        .iter()
        .find(|glyph| glyph.text == " ")
        .expect("a space");
    assert!(
        (space.advance - 6.0 / 9.0 * space.size).abs() < 1e-9,
        "the space advances like every other fallback glyph"
    );
}

#[test]
fn an_embedded_serif_advances_each_character_by_its_own_width() {
    let page = traced(BOOK);
    let advances: Vec<f64> = page
        .glyphs
        .iter()
        .filter(|glyph| glyph.text.trim().chars().count() == 1)
        .map(|glyph| (glyph.advance * 1000.0).round() / 1000.0)
        .collect();
    assert!(advances.len() > 20, "the book page has text: {advances:?}");
    let mut distinct = advances.clone();
    distinct.sort_by(|left, right| left.partial_cmp(right).expect("finite"));
    distinct.dedup();
    assert!(
        distinct.len() > 5,
        "a serif's advances differ per glyph, got {distinct:?}"
    );
    assert!(
        advances.iter().any(|advance| *advance > 0.55),
        "an em-wide glyph exists: {advances:?}"
    );
}

#[test]
fn stated_widths_are_what_a_glyph_advances_by() {
    // `book.pdf` states `/Widths` for its font (finding (3)): the pen must follow
    // them rather than the font's own advance, so every box's advance is the
    // width the file declares for that code.
    let document = Document::open(BOOK).expect("the fixture opens");
    let page = document.page(0).expect("the page");
    let resources = page.resources.expect("the page has resources");
    let fonts = follow(&document, resources.get(b"Font").expect("/Font"));
    let Object::Dictionary(fonts) = fonts else {
        panic!("/Font is not a dictionary");
    };
    let (_, font) = fonts.iter().next().expect("a font resource");
    let font = follow(&document, font);
    let Object::Dictionary(font) = font else {
        panic!("the font is not a dictionary");
    };
    let first = font
        .get(b"FirstChar")
        .and_then(Object::as_integer)
        .unwrap_or(0);
    let Object::Array(widths) = follow(&document, font.get(b"Widths").expect("/Widths")) else {
        panic!("/Widths is not an array");
    };

    let traced = traced(BOOK);
    let mut checked = 0;
    for glyph in &traced.glyphs {
        let Some(character) = glyph.text.chars().next() else {
            continue;
        };
        if glyph.text.chars().count() != 1 || !character.is_ascii_graphic() {
            continue;
        }
        let index = i64::from(character as u32) - first;
        let Some(width) = usize::try_from(index).ok().and_then(|i| widths.get(i)) else {
            continue;
        };
        let expected = width.as_real().expect("a number") / 1000.0 * glyph.size;
        assert!(
            (glyph.advance - expected).abs() < 1e-9,
            "{:?} advances {} but /Widths says {}",
            glyph.text,
            glyph.advance,
            expected
        );
        checked += 1;
    }
    assert!(
        checked > 20,
        "the page's /Widths were actually compared: {checked}"
    );
}

/// Follow a value that may be an indirect reference to the object itself.
fn follow(document: &Document, value: &Object) -> Object {
    match value {
        Object::Reference(id) => document.get_object(*id).expect("the object reads"),
        other => other.clone(),
    }
}

#[test]
fn the_untraced_render_is_the_same_raster() {
    // `render_page` is `paint_page` with the glyphs dropped: the raster must not
    // change because something asked for the boxes as well.
    let document = Document::open(TEXT).expect("the fixture opens");
    let plain = render_page_number(&document, 0, &RenderOptions::default()).expect("renders");
    let with_glyphs =
        render_page_number_traced(&document, 0, &RenderOptions::default()).expect("renders");
    assert_eq!(plain.canvas.width(), with_glyphs.page.canvas.width());
    assert_eq!(plain.canvas.height(), with_glyphs.page.canvas.height());
    assert!(
        plain.canvas.pixels() == with_glyphs.page.canvas.pixels(),
        "the traced render painted a different page"
    );
    assert_eq!(plain.report, with_glyphs.page.report);
}

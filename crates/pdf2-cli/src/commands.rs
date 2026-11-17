//! The four file commands. Each returns [`Failure`] rather than printing an
//! error, so the exit code is decided in one place.

use std::path::Path;

use pdf2::{Document, ObjectId, Rect};

use crate::args::{Failure, Run, Selection};
use crate::json;

/// Open the file named on the command line, with its password.
pub fn open(run: &Run) -> Result<Document, Failure> {
    let bytes = std::fs::read(&run.file).map_err(|error| at(run, error))?;
    Document::open_with_password(bytes, &run.password).map_err(|error| at(run, error))
}

/// `info`: document version, page count, encryption and metadata state.
pub fn info(run: &Run) -> Result<(), Failure> {
    let document = open(run)?;
    let pages = document.page_count().map_err(|error| at(run, error))?;
    let metadata = document.has_metadata().map_err(|error| at(run, error))?;
    if run.json {
        println!(
            "{{\"file\":{},\"version\":{},\"pages\":{pages},\"encrypted\":{},\"metadata\":{metadata}}}",
            json::string(&display(run)),
            json::string(document.version()),
            document.is_encrypted(),
        );
    } else {
        println!("file: {}", display(run));
        println!("version: {}", document.version());
        println!("pages: {pages}");
        println!("encrypted: {}", yes_no(document.is_encrypted()));
        println!("metadata: {}", yes_no(metadata));
    }
    Ok(())
}

/// `pages`: one entry per page with its object id, boxes, rotation and content
/// streams.
pub fn pages(run: &Run) -> Result<(), Failure> {
    let document = open(run)?;
    let pages = document.pages().map_err(|error| at(run, error))?;
    if run.json {
        let entries: Vec<String> = pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                format!(
                    "{{\"index\":{},\"object\":{},\"mediaBox\":{},\"cropBox\":{},\"rotate\":{},\"contents\":[{}]}}",
                    index + 1,
                    match page.id {
                        Some(id) => json::string(&object_text(id)),
                        None => "null".to_owned(),
                    },
                    json::numbers(page.media_box.map(rect_array)),
                    json::numbers(page.crop_box.map(rect_array)),
                    page.rotate,
                    page.contents
                        .iter()
                        .map(|id| json::string(&object_text(*id)))
                        .collect::<Vec<_>>()
                        .join(","),
                )
            })
            .collect();
        println!(
            "{{\"file\":{},\"pages\":[{}]}}",
            json::string(&display(run)),
            entries.join(",")
        );
    } else {
        for (index, page) in pages.iter().enumerate() {
            println!("page {}", index + 1);
            println!("  object: {}", object_text_or_dash(page.id));
            println!("  media box: {}", box_text(page.media_box));
            println!("  crop box: {}", box_text(page.crop_box));
            println!("  rotate: {}", page.rotate);
            println!(
                "  contents: {}",
                if page.contents.is_empty() {
                    "-".to_owned()
                } else {
                    page.contents
                        .iter()
                        .map(|id| object_text(*id))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
    }
    Ok(())
}

/// `text`: extracted text for the selected pages.
pub fn text(run: &Run, selection: Selection) -> Result<(), Failure> {
    let document = open(run)?;
    let count = document.page_count().map_err(|error| at(run, error))?;
    let indexes: Vec<usize> = match selection {
        Selection::All => (1..=count).collect(),
        Selection::Page(page) => vec![page],
    };
    let mut extracted = Vec::with_capacity(indexes.len());
    for index in indexes {
        if index > count {
            return Err(Failure::input(format!(
                "{}: page {index} does not exist (the document has {count})",
                display(run)
            )));
        }
        let page = document.page(index - 1).map_err(|error| at(run, error))?;
        let page_text = document
            .extract_text(&page)
            .map_err(|error| at(run, error))?;
        extracted.push((index, page_text));
    }
    if run.json {
        let entries: Vec<String> = extracted
            .iter()
            .map(|(index, page_text)| {
                format!("{{\"index\":{index},\"text\":{}}}", json::string(page_text))
            })
            .collect();
        println!(
            "{{\"file\":{},\"pages\":[{}]}}",
            json::string(&display(run)),
            entries.join(",")
        );
    } else {
        for (position, (_, page_text)) in extracted.iter().enumerate() {
            if position > 0 {
                println!();
            }
            print!("{page_text}");
            if !page_text.ends_with('\n') {
                println!();
            }
        }
    }
    Ok(())
}

/// `check`: open, walk, decode and extract every page, and report what fails.
pub fn check(run: &Run) -> Result<(), Failure> {
    let document = open(run)?;
    let pages = document.pages().map_err(|error| at(run, error))?;
    let mut failures: Vec<(usize, String)> = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        let number = index + 1;
        if let Err(error) = document.page_content(page) {
            failures.push((number, format!("content: {error}")));
            continue;
        }
        if let Err(error) = document.extract_text(page) {
            failures.push((number, format!("text: {error}")));
        }
    }
    let ok = failures.is_empty();
    if run.json {
        let entries: Vec<String> = failures
            .iter()
            .map(|(page, error)| format!("{{\"page\":{page},\"error\":{}}}", json::string(error)))
            .collect();
        println!(
            "{{\"file\":{},\"pages\":{},\"ok\":{ok},\"failures\":[{}]}}",
            json::string(&display(run)),
            pages.len(),
            entries.join(",")
        );
    } else if ok {
        println!("ok: {}: {} page(s) readable", display(run), pages.len());
    } else {
        for (page, error) in &failures {
            println!("page {page}: {error}");
        }
        println!(
            "failed: {}: {} of {} page(s) unreadable",
            display(run),
            failures.len(),
            pages.len()
        );
    }
    if ok {
        Ok(())
    } else {
        Err(Failure::input(format!(
            "{}: {} page(s) failed",
            display(run),
            failures.len()
        )))
    }
}

fn at(run: &Run, error: impl std::fmt::Display) -> Failure {
    Failure::input(format!("{}: {error}", display(run)))
}

fn display(run: &Run) -> String {
    path_text(&run.file)
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn object_text(id: ObjectId) -> String {
    format!("{} {}", id.number, id.generation)
}

fn object_text_or_dash(id: Option<ObjectId>) -> String {
    id.map_or_else(|| "-".to_owned(), object_text)
}

fn rect_array(rect: Rect) -> [f64; 4] {
    [rect.x0, rect.y0, rect.x1, rect.y1]
}

fn box_text(rect: Option<Rect>) -> String {
    match rect {
        Some(rect) => format!(
            "{} {} {} {}",
            number_text(rect.x0),
            number_text(rect.y0),
            number_text(rect.x1),
            number_text(rect.y1)
        ),
        None => "none".to_owned(),
    }
}

fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.is_finite() {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::{box_text, number_text, object_text_or_dash};
    use pdf2::{ObjectId, Rect};

    #[test]
    fn whole_numbers_print_without_a_decimal_point() {
        assert_eq!(number_text(612.0), "612");
        assert_eq!(number_text(0.5), "0.5");
    }

    #[test]
    fn boxes_render_as_four_numbers_or_none() {
        assert_eq!(
            box_text(Some(Rect::new(0.0, 0.0, 612.0, 792.0))),
            "0 0 612 792"
        );
        assert_eq!(box_text(None), "none");
    }

    #[test]
    fn a_direct_page_has_no_object_id() {
        assert_eq!(object_text_or_dash(None), "-");
        assert_eq!(object_text_or_dash(Some(ObjectId::new(5, 0))), "5 0");
    }
}

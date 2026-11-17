//! The corpus: the committed fixtures, generated stress files, and an optional
//! directory of real-world files (`PDF2_CORPUS`).

use std::path::{Path, PathBuf};

use pdf2::{Document, Rect};

/// One measured set of PDF files.
pub struct Group {
    /// Short name used to prefix metric keys, e.g. `fixtures` or `pages-100`.
    pub name: String,
    /// The files, already read into memory.
    pub files: Vec<Vec<u8>>,
}

impl Group {
    /// Total size of the group in bytes.
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|file| file.len() as u64).sum()
    }
}

/// Build every group. Generated files are written under `scratch` and left
/// there for inspection; the directory is a temporary one owned by the caller.
pub fn build(scratch: &Path) -> Result<Vec<Group>, String> {
    let mut groups = vec![Group {
        name: "fixtures".to_owned(),
        files: fixture_files()?,
    }];
    for (name, bytes) in generated_files()? {
        std::fs::write(scratch.join(format!("{name}.pdf")), &bytes)
            .map_err(|error| format!("writing {name}.pdf: {error}"))?;
        groups.push(Group {
            name,
            files: vec![bytes],
        });
    }
    if let Some(group) = corpus_dir()? {
        groups.push(group);
    }
    Ok(groups)
}

/// The committed fixtures, excluding the encrypted one: it needs a password,
/// which the generic measurement loop does not have.
fn fixture_files() -> Result<Vec<Vec<u8>>, String> {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name != "encrypted-r6.pdf")
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| std::fs::read(path).map_err(|error| format!("{}: {error}", path.display())))
        .collect()
}

/// Stress files generated with this crate's own writer.
fn generated_files() -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut files = Vec::new();
    for pages in [1_u32, 100, 1000] {
        files.push((format!("pages-{pages}"), document_with_pages(pages, 1)?));
    }
    files.push(("stream-1mb".to_owned(), document_with_stream(1024 * 1024)?));
    // Rank 8's budget file: 65 000 objects is more than a large real document
    // holds, and it is generated here rather than committed — it is four
    // megabytes of table, and the committed fixtures are the benchmark's input
    // set, so a copy in `tests/fixtures` would be read by every other measurement.
    files.push(("objects-65000".to_owned(), document_with_objects(65_000)?));
    Ok(files)
}

fn document_with_pages(pages: u32, _marker: u8) -> Result<Vec<u8>, String> {
    let mut document = Document::new();
    for page in 0..pages {
        let content = format!("BT /F1 12 Tf 72 700 Td (page {page}) Tj ET");
        document.add_page(Rect::new(0.0, 0.0, 612.0, 792.0), content);
    }
    document.save().map_err(|error| error.to_string())
}

/// A one-page document with `count` further objects in its cross-reference: the
/// shape a huge real-world catalogue has, and what the object ceiling is about.
fn document_with_objects(count: usize) -> Result<Vec<u8>, String> {
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::with_capacity(count + 4);
    let mut object = |number: usize, body: &[u8], out: &mut Vec<u8>| {
        offsets.push((number, out.len()));
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    };
    // Paint, not text: the point of the file is its object table, and a text run
    // would also need a font resource to be measured as text.
    let content = b"0 0 1 rg 72 700 100 50 re f";
    object(1, b"<< /Type /Catalog /Pages 2 0 R >>", &mut out);
    object(2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>", &mut out);
    object(
        3,
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>",
        &mut out,
    );
    object(
        4,
        format!(
            "<< /Length {} >>\nstream\n{}\nendstream",
            content.len(),
            core::str::from_utf8(content).map_err(|error| error.to_string())?
        )
        .as_bytes(),
        &mut out,
    );
    // The filler: objects nothing refers to, which a real document has by the
    // thousand (struct tree elements, annotations, fonts it no longer uses).
    for extra in 0..count {
        let number = 5 + extra;
        object(number, b"<< /Filler true >>", &mut out);
    }
    let xref_at = out.len();
    let total = 5 + count;
    out.extend_from_slice(format!("xref\n0 {total}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for (_, offset) in &offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {total} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    Ok(out)
}

fn document_with_stream(size: usize) -> Result<Vec<u8>, String> {
    const PREFIX: &[u8] = b"BT /F1 12 Tf 72 700 Td (";
    const SUFFIX: &[u8] = b") Tj ET";
    let body = size.saturating_sub(PREFIX.len() + SUFFIX.len());
    let mut content = Vec::with_capacity(PREFIX.len() + body + SUFFIX.len());
    content.extend_from_slice(PREFIX);
    content.resize(PREFIX.len() + body, b'a');
    content.extend_from_slice(SUFFIX);
    let mut document = Document::new();
    document.add_page(Rect::new(0.0, 0.0, 612.0, 792.0), content);
    document.save().map_err(|error| error.to_string())
}

/// Files from `PDF2_CORPUS`, when that environment variable points at a
/// directory of PDFs. They are measured but are not part of the committed
/// baseline, so they cannot make the regression check fail.
fn corpus_dir() -> Result<Option<Group>, String> {
    let Some(directory) = std::env::var_os("PDF2_CORPUS") else {
        return Ok(None);
    };
    let directory = PathBuf::from(directory);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .collect();
    paths.sort();
    let mut files = Vec::new();
    for path in paths {
        files.push(std::fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?);
    }
    if files.is_empty() {
        return Ok(None);
    }
    Ok(Some(Group {
        name: "corpus".to_owned(),
        files,
    }))
}

#[cfg(test)]
mod tests {
    use super::{document_with_objects, document_with_pages, document_with_stream, fixture_files};
    use pdf2::Document;

    #[test]
    fn generated_documents_open_and_have_the_expected_pages() {
        for pages in [1_u32, 100] {
            let bytes = document_with_pages(pages, 1).expect("generated");
            let document = Document::open(bytes).expect("opens");
            assert_eq!(document.page_count().expect("pages"), pages as usize);
        }
    }

    #[test]
    fn the_budget_document_holds_sixty_five_thousand_objects() {
        let bytes = document_with_objects(65_000).expect("generated");
        let document = Document::open(bytes).expect("opens");
        assert_eq!(document.page_count().expect("pages"), 1);
        // Every object is reachable: the point of the file is that reading them
        // is bounded work, not that the tree is small.
        let last = document
            .get_object_by_number(65_004)
            .expect("the last filler object");
        assert!(matches!(last, pdf2::Object::Dictionary(_)));
        // And the ceiling refuses reading more than the budget allows, which is
        // what makes the file's own size a decision rather than a hazard. The
        // budget counts what is *held*, so it fills up as objects are read.
        document.set_object_limit(0);
        let error = document
            .get_object_by_number(65_000)
            .expect_err("nothing may be held");
        assert!(matches!(error, pdf2::Error::TooMany { .. }), "{error:?}");
    }

    #[test]
    fn the_stream_document_carries_about_a_megabyte() {
        let bytes = document_with_stream(1024 * 1024).expect("generated");
        assert!(bytes.len() > 1024 * 1024, "{}", bytes.len());
        let document = Document::open(bytes).expect("opens");
        let page = document.page(0).expect("page");
        let content = document.page_content(&page).expect("content");
        assert_eq!(content.len(), 1024 * 1024);
    }

    #[test]
    fn the_fixture_group_holds_the_plain_fixtures_in_order() {
        let files = fixture_files().expect("fixtures");
        assert!(files.len() >= 8, "the plain fixtures: {}", files.len());
        for file in &files {
            // Every fixture is 2.0 except the revision-4 encryption ones, which
            // say 1.7 because a 2.0 header with `/V 4 /R 4` is a contradiction.
            let modern = file.starts_with(b"%PDF-2.0");
            let legacy_r4 = file.starts_with(b"%PDF-1.7")
                && file.array_windows::<4>().any(|window| window == b"/R 4");
            assert!(modern || legacy_r4, "a committed fixture");
        }
    }
}

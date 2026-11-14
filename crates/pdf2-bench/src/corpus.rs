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
    use super::{document_with_pages, document_with_stream, fixture_files};
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
            assert!(file.starts_with(b"%PDF-2.0"), "a committed fixture");
        }
    }
}

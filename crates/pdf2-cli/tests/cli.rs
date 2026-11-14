//! The binary as a user runs it: `CARGO_BIN_EXE_pdf2` plus the committed
//! fixtures, so these tests fail if the shipped command misbehaves rather than
//! if an internal function does.
//!
//! Exit codes are part of the contract: 0 success, 1 bad command line, 2 the
//! file could not be read, decrypted, parsed or interpreted.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Every plain fixture; `encrypted-r6.pdf` needs a password and is handled
/// separately.
const PLAIN_FIXTURES: &[&str] = &[
    "classic-xref.pdf",
    "classic-twin.pdf",
    "xref-stream.pdf",
    "xref-stream-prev.pdf",
    "hybrid-xref.pdf",
    "page-tree.pdf",
    "text.pdf",
    "plain-twin.pdf",
];

fn binary() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pdf2"))
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn path(name: &str) -> String {
    fixture(name).to_str().expect("utf-8 path").to_owned()
}

fn run(args: &[&str]) -> Output {
    binary().args(args).output().expect("spawn pdf2")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(output)
    );
}

fn assert_exit(output: &Output, code: i32) {
    assert_eq!(
        output.status.code(),
        Some(code),
        "stdout: {}, stderr: {}",
        stdout(output),
        stderr(output)
    );
}

/// Write bytes to a unique temporary file and return (path, guard).
fn temp_file(name: &str, bytes: &[u8]) -> (PathBuf, TempFile) {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "pdf2-cli-test-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::write(&path, bytes).expect("write temp fixture");
    let guard = TempFile(path.clone());
    (path, guard)
}

struct TempFile(PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).expect("read fixture")
}

fn case(path: &Path) -> String {
    path.to_str().expect("utf-8 path").to_owned()
}

#[test]
fn info_reports_a_plain_document() {
    let output = run(&["info", &path("text.pdf")]);
    assert_success(&output);
    let text = stdout(&output);
    assert!(text.contains("version: 2.0"), "{text}");
    assert!(text.contains("pages: 1"), "{text}");
    assert!(text.contains("encrypted: no"), "{text}");
    assert!(text.contains("metadata: no"), "{text}");
}

#[test]
fn info_json_is_machine_readable() {
    let output = run(&[
        "info",
        &path("encrypted-r6.pdf"),
        "--password",
        "user",
        "--json",
    ]);
    assert_success(&output);
    let text = stdout(&output);
    assert!(
        text.starts_with('{') && text.trim_end().ends_with('}'),
        "{text}"
    );
    assert!(text.contains("\"version\":\"2.0\""), "{text}");
    assert!(text.contains("\"pages\":1"), "{text}");
    assert!(text.contains("\"encrypted\":true"), "{text}");
    assert!(text.contains("\"metadata\":true"), "{text}");
}

#[test]
fn info_opens_an_encrypted_document_with_its_password() {
    let output = run(&["info", &path("encrypted-r6.pdf"), "--password", "user"]);
    assert_success(&output);
    let text = stdout(&output);
    assert!(text.contains("encrypted: yes"), "{text}");
    assert!(text.contains("metadata: yes"), "{text}");
}

#[test]
fn pages_lists_every_page_with_its_boxes() {
    let output = run(&["pages", &path("page-tree.pdf")]);
    assert_success(&output);
    let text = stdout(&output);
    for expected in [
        "page 1",
        "object: 5 0",
        "media box: 0 0 200 300",
        "rotate: 90",
        "contents: 4 0",
        "page 2",
        "crop box: 10 20 190 280",
        "page 3",
        "rotate: 180",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
}

#[test]
fn pages_json_reports_the_same_facts() {
    let output = run(&["pages", &path("page-tree.pdf"), "--json"]);
    assert_success(&output);
    let text = stdout(&output);
    for expected in [
        "\"index\":1",
        "\"object\":\"5 0\"",
        "\"mediaBox\":[0, 0, 200, 300]",
        "\"cropBox\":[10, 20, 190, 280]",
        "\"rotate\":180",
        "\"contents\":[\"4 0\"]",
    ] {
        assert!(text.contains(expected), "missing {expected:?} in {text}");
    }
}

#[test]
fn text_prints_the_first_page_by_default() {
    let output = run(&["text", &path("text.pdf")]);
    assert_success(&output);
    let text = stdout(&output);
    assert!(text.contains("Hello, PDF 2.0"), "{text}");
    assert!(text.contains("caf\u{e9} ok"), "{text}");
}

#[test]
fn text_can_select_a_page_or_every_page() {
    let first = run(&["text", &path("text.pdf"), "--page", "1"]);
    assert_success(&first);
    assert_eq!(stdout(&first), stdout(&run(&["text", &path("text.pdf")])));

    let all = run(&["text", &path("page-tree.pdf"), "--all"]);
    assert_success(&all);
    assert!(stdout(&all).contains("page one"), "{}", stdout(&all));

    let json = run(&["text", &path("text.pdf"), "--json"]);
    assert_success(&json);
    let text = stdout(&json);
    assert!(text.contains("\"index\":1"), "{text}");
    assert!(text.contains("Hello, PDF 2.0"), "{text}");
}

#[test]
fn check_accepts_every_committed_fixture() {
    for name in PLAIN_FIXTURES {
        let output = run(&["check", &path(name)]);
        assert!(
            output.status.success(),
            "{name}: exit {:?}, {}",
            output.status.code(),
            stderr(&output)
        );
        assert!(
            stdout(&output).starts_with("ok: "),
            "{name}: {}",
            stdout(&output)
        );
    }
    let output = run(&["check", &path("encrypted-r6.pdf"), "--password", "user"]);
    assert_success(&output);
    assert!(stdout(&output).starts_with("ok: "), "{}", stdout(&output));
}

#[test]
fn check_json_reports_its_verdict() {
    let output = run(&["check", &path("page-tree.pdf"), "--json"]);
    assert_success(&output);
    let text = stdout(&output);
    assert!(text.contains("\"pages\":3"), "{text}");
    assert!(text.contains("\"ok\":true"), "{text}");
    assert!(text.contains("\"failures\":[]"), "{text}");
}

#[test]
fn a_wrong_password_is_an_input_error() {
    let output = run(&["info", &path("encrypted-r6.pdf"), "--password", "wrong"]);
    assert_exit(&output, 2);
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    assert!(
        stderr(&output).starts_with("error: "),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_truncated_file_is_an_input_error() {
    let bytes = fixture_bytes("text.pdf");
    let (path, _guard) = temp_file("truncated.pdf", &bytes[..100]);
    let output = run(&["check", &case(&path)]);
    assert_exit(&output, 2);
    assert!(
        stderr(&output).starts_with("error: "),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_missing_file_is_an_input_error() {
    let output = run(&["info", "/no/such/file.pdf"]);
    assert_exit(&output, 2);
    assert!(
        stderr(&output).contains("/no/such/file.pdf"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn a_page_that_does_not_exist_is_an_input_error() {
    let output = run(&["text", &path("text.pdf"), "--page", "9"]);
    assert_exit(&output, 2);
    assert!(
        stderr(&output).contains("does not exist"),
        "{}",
        stderr(&output)
    );
}

#[test]
fn bad_command_lines_exit_one() {
    for arguments in [
        vec![],
        vec!["frobnicate"],
        vec!["info"],
        vec!["info", "a.pdf", "--nope"],
        vec!["text", "a.pdf", "--page", "0"],
        vec!["text", "a.pdf", "--all", "--page", "2"],
    ] {
        let output = binary().args(&arguments).output().expect("spawn pdf2");
        assert_exit(&output, 1);
        assert!(
            stderr(&output).starts_with("error: "),
            "{arguments:?}: {}",
            stderr(&output)
        );
    }
}

#[test]
fn help_and_version_exit_zero() {
    for flag in ["--help", "-h", "help"] {
        let output = run(&[flag]);
        assert_success(&output);
        let text = stdout(&output);
        assert!(text.contains("pdf2 info <file>"), "{flag}: {text}");
        assert!(text.contains("Exit codes:"), "{flag}: {text}");
    }
    let output = run(&["--version"]);
    assert_success(&output);
    assert!(
        stdout(&output).starts_with("pdf2-cli 0.1.0-alpha.1"),
        "{}",
        stdout(&output)
    );
}

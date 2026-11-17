//! Repository chores, so that "run the gate" is one command rather than a
//! remembered sequence of five.
//!
//! ```text
//! cargo gate                 # what the CI Gate job runs
//! cargo gate --full          # plus no_std, wasm and the benchmark budget
//! cargo fixtures             # regenerate tests/fixtures/*.pdf
//! cargo bless                # re-render the committed goldens
//! ```
//!
//! `--full` needs the `wasm32-unknown-unknown`, `wasm32-wasip1` and
//! `thumbv7em-none-eabihf` targets and `wasm-bindgen-cli` 0.2.127 installed;
//! each step says what to install when it is missing.

use std::process::{Command, ExitCode};

const GATE_STEPS: &[&str] = &["format", "clippy", "test", "features", "docs", "ledger"];

fn main() -> ExitCode {
    // Every chore names paths relative to the workspace root (`docs/…`,
    // `corpus/…`, `crates/…`), and `cargo run` keeps the caller's directory, so
    // a gate run from a subdirectory used to fail on a missing file. Resolve the
    // root from this crate's manifest and work from there.
    if let Err(error) = enter_workspace_root() {
        eprintln!("error: {error}");
        return ExitCode::from(1);
    }
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    let full = args.iter().any(|arg| arg == "--full");
    args.retain(|arg| arg != "--full");
    let command = args.first().map_or("gate", String::as_str);

    let result = match command {
        "gate" => gate(full),
        "format" => step("format"),
        "clippy" => step("clippy"),
        "test" => step("test"),
        "docs" => step("docs"),
        "ledger" => ledger(),
        "no-std" => no_std(),
        "wasm" => wasm(),
        "wasm-package" => wasm_package(),
        "demo" => demo(),
        "corpus" => corpus(),
        "tier-b" => tier_b(&args[1..]),
        "bench" => bench(),
        "fixtures" => fixtures(),
        "cmaps" => cmaps(),
        "bless" => bless(),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            Ok(())
        }
        other => Err(format!("unknown chore `{other}`\n\n{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

const USAGE: &str = "\
cargo xtask — repository chores

Usage:
  cargo gate [--full]   run every gate step (CI's Gate job; --full adds the rest)
  cargo xtask format    check formatting only
  cargo xtask clippy    clippy over the workspace, warnings denied
  cargo xtask test      test the workspace and the library's features
  cargo xtask docs      rustdoc over the workspace, warnings denied
  cargo xtask no-std    build and lint the library without std
  cargo xtask wasm      build the wasm modules, smoke test, check size budgets
  cargo xtask wasm-package
                        build the npm package into crates/pdf2-wasm/pkg
  cargo xtask demo      build the demo and drive it in a real browser
  cargo xtask corpus    run the Tier A corpus through the CLI and the renderer
  cargo xtask tier-b <dir>...
                        measure a Tier B tree and write a failure histogram
  cargo xtask ledger    check that docs/FEATURES.md and the roadmap agree
  cargo xtask bench     measure and compare with docs/performance-baseline.json
  cargo xtask fixtures  regenerate tests/fixtures/*.pdf
  cargo run -q -p xtask -- cmaps
                        rebuild the generated CMap tables from vendor/adobe-cmaps
  cargo xtask bless     re-render the committed golden PNGs

Exit codes: 0 all good, 1 a step failed.";

/// Change into the workspace root, where every chore's paths are relative.
fn enter_workspace_root() -> Result<(), String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root
        .canonicalize()
        .map_err(|error| format!("workspace root {}: {error}", root.display()))?;
    std::env::set_current_dir(&root)
        .map_err(|error| format!("changing to {}: {error}", root.display()))
}

fn gate(full: bool) -> Result<(), String> {
    println!(
        "gate: {} steps{}",
        GATE_STEPS.len(),
        if full { ", then --full" } else { "" }
    );
    for name in GATE_STEPS {
        step(name)?;
    }
    if full {
        no_std()?;
        wasm()?;
        demo()?;
        bench()?;
    }
    println!("\ngate: all steps passed");
    Ok(())
}

fn step(name: &str) -> Result<(), String> {
    match name {
        "format" => run("cargo", &["fmt", "--all", "--check"]),
        "clippy" => run(
            "cargo",
            &[
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
        ),
        "test" => run("cargo", &["test", "--workspace"]),
        "features" => run("cargo", &["test", "-p", "pdf2", "--all-features"]),
        "docs" => run_env(
            "cargo",
            &["doc", "--workspace", "--no-deps", "--lib"],
            &[("RUSTDOCFLAGS", "-D warnings")],
        ),
        "ledger" => ledger(),
        other => Err(format!("unknown gate step `{other}`")),
    }
}

fn no_std() -> Result<(), String> {
    for target in ["x86_64-unknown-linux-gnu", "thumbv7em-none-eabihf"] {
        run(
            "cargo",
            &[
                "build",
                "-p",
                "pdf2",
                "--no-default-features",
                "--features",
                "alloc",
                "--target",
                target,
            ],
        )?;
    }
    run(
        "cargo",
        &[
            "clippy",
            "-p",
            "pdf2",
            "--no-default-features",
            "--features",
            "alloc",
            "--",
            "-D",
            "warnings",
        ],
    )
}

fn wasm() -> Result<(), String> {
    run(
        "cargo",
        &[
            "build",
            "-p",
            "pdf2-wasm-smoke",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--lib",
        ],
    )?;
    run(
        "cargo",
        &[
            "build",
            "-p",
            "pdf2-wasm-smoke",
            "--release",
            "--target",
            "wasm32-wasip1",
            "--bin",
            "pdf2-wasi-smoke",
        ],
    )?;
    run(
        "node",
        &["--no-warnings", "crates/pdf2-wasm-smoke/smoke.mjs"],
    )?;
    wasm_package()
}

/// Build what npm publishes: the bindgen web output at the package root, with
/// the manifest, README and licences beside it, and no demo directory. The demo
/// site gets its own copy of the two files it needs to load.
fn wasm_package() -> Result<(), String> {
    const WASM: &str = "target/wasm32-unknown-unknown/release/pdf2_wasm.wasm";
    run(
        "cargo",
        &[
            "build",
            "-p",
            "pdf2-wasm",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
        ],
    )?;
    // Node glue, for the API smoke test only; not part of the package.
    run(
        "wasm-bindgen",
        &[
            "--target",
            "nodejs",
            "--out-dir",
            "crates/pdf2-wasm/pkg-node",
            WASM,
        ],
    )?;
    // The published package: TypeScript declarations included.
    run(
        "wasm-bindgen",
        &["--target", "web", "--out-dir", "crates/pdf2-wasm/pkg", WASM],
    )?;
    for (from, to) in [
        (
            "crates/pdf2-wasm/package.json",
            "crates/pdf2-wasm/pkg/package.json",
        ),
        (
            "crates/pdf2-wasm/README.md",
            "crates/pdf2-wasm/pkg/README.md",
        ),
        ("LICENSE-MIT", "crates/pdf2-wasm/pkg/LICENSE-MIT"),
        (
            "LICENSE-UNLICENSE",
            "crates/pdf2-wasm/pkg/LICENSE-UNLICENSE",
        ),
    ] {
        copy(from, to)?;
    }
    // The demo's artefact is **not** the npm one: the decoders are non-default
    // features (ADR 0007), so the site is built with them on while the package
    // stays lean. Two builds, two budgets — otherwise a demo that claims a codec
    // silently shows a blank page.
    run(
        "cargo",
        &[
            "build",
            "-p",
            "pdf2-wasm",
            "--release",
            "--target",
            "wasm32-unknown-unknown",
            "--features",
            "jpx,jbig2",
        ],
    )?;
    run(
        "wasm-bindgen",
        &[
            "--target",
            "web",
            "--no-typescript",
            "--out-dir",
            "crates/pdf2-wasm/demo/pkg",
            WASM,
        ],
    )?;
    // GitHub Pages serves fixed file names with a cache lifetime, so a browser
    // can pair a new page with last week's wasm and render nothing where the
    // newer build would draw. The demo imports the module with this stamp.
    let version = std::fs::read_to_string("crates/pdf2-wasm/package.json")
        .ok()
        .and_then(|text| {
            text.split("\"version\": \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next().map(str::to_owned))
        })
        .unwrap_or_else(|| "0.0.0".to_owned());
    let build = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    std::fs::write(
        "crates/pdf2-wasm/demo/build.json",
        format!("{{\"version\":\"{version}\",\"build\":\"{build}\"}}\n"),
    )
    .map_err(|error| format!("writing build.json: {error}"))?;
    // The demo's samples are the crate's own fixtures, copied rather than kept:
    // a committed copy went stale the moment the fixture grew pages, and the
    // page numbers in the feature list began to point past the end of it.
    for (from, to) in [
        (
            "tests/fixtures/text.pdf",
            "crates/pdf2-wasm/demo/samples/text.pdf",
        ),
        (
            "tests/fixtures/paths.pdf",
            "crates/pdf2-wasm/demo/samples/paths.pdf",
        ),
        (
            "tests/fixtures/images.pdf",
            "crates/pdf2-wasm/demo/samples/images.pdf",
        ),
        (
            "tests/fixtures/calibration.pdf",
            "crates/pdf2-wasm/demo/samples/calibration.pdf",
        ),
        (
            "tests/fixtures/book.pdf",
            "crates/pdf2-wasm/demo/samples/book.pdf",
        ),
        (
            "tests/fixtures/encrypted-r6.pdf",
            "crates/pdf2-wasm/demo/samples/encrypted.pdf",
        ),
        // The 1.2 samples: what the release added, visible rather than claimed.
        (
            "tests/fixtures/encrypted-r4-aes.pdf",
            "crates/pdf2-wasm/demo/samples/encrypted-legacy.pdf",
        ),
        (
            "tests/fixtures/cropbox.pdf",
            "crates/pdf2-wasm/demo/samples/cropbox.pdf",
        ),
        (
            "corpus/tier-a/cid_cff.pdf",
            "crates/pdf2-wasm/demo/samples/cff.pdf",
        ),
        (
            "corpus/tier-a/90ms_rksj_h_sample.pdf",
            "crates/pdf2-wasm/demo/samples/cjk.pdf",
        ),
        // The 1.3 samples: the painting family, which 1.3 is about. Each is one
        // page a reader can look at and see the feature rather than read about it.
        (
            "tests/fixtures/shading.pdf",
            "crates/pdf2-wasm/demo/samples/shading.pdf",
        ),
        (
            "tests/fixtures/blend-modes.pdf",
            "crates/pdf2-wasm/demo/samples/blend-modes.pdf",
        ),
        (
            "tests/fixtures/type1.pdf",
            "crates/pdf2-wasm/demo/samples/type1.pdf",
        ),
        // The 1.4 samples: the three painting features, one page each, plus the
        // Type 3 page that shows what happens when a clip has no outline to take.
        // The tiling showcase is the corpus page, not the crate's own fixture:
        // `tiling.pdf` paints a 15x20 pt lattice to keep its pixel assertions
        // exact, which is a nearly blank page in a 200 pt viewer, while
        // `colorkeymask.pdf` fills a 200x267 rectangle with one cell.
        (
            "corpus/tier-a/colorkeymask.pdf",
            "crates/pdf2-wasm/demo/samples/colorkeymask.pdf",
        ),
        // The decoder samples: the demo build carries the codecs (ADR 0007), so
        // the site can show a JPEG 2000 image and a JBIG2 scan rather than claim
        // them. Both are pdf.js corpus files, like the ones above.
        (
            "corpus/tier-a/bug_jpx.pdf",
            "crates/pdf2-wasm/demo/samples/jpx.pdf",
        ),
        (
            "corpus/tier-a/bitmap-composite-and-xnor.pdf",
            "crates/pdf2-wasm/demo/samples/jbig2-scan.pdf",
        ),
        (
            "tests/fixtures/colour-key.pdf",
            "crates/pdf2-wasm/demo/samples/colour-key.pdf",
        ),
        (
            "tests/fixtures/tr-clip.pdf",
            "crates/pdf2-wasm/demo/samples/tr-clip.pdf",
        ),
        (
            "tests/fixtures/tr-clip-type3.pdf",
            "crates/pdf2-wasm/demo/samples/tr-clip-type3.pdf",
        ),
    ] {
        copy(from, to)?;
    }
    // Five samples come from pdf.js's corpus (Apache-2.0), so the page ships the
    // attribution beside them rather than only in the repository.
    std::fs::write(
        "crates/pdf2-wasm/demo/samples/NOTICE.md",
        "`cff.pdf`, `cjk.pdf`, `colorkeymask.pdf`, `jpx.pdf` and `jbig2-scan.pdf` are from\n\
         the pdf.js test corpus,\n\
         commit d60bc7426aa0968d5b95ff8ca3a76dbc6ce23a3a, used under the\n\
         Apache License 2.0 (`corpus/LICENSES/Apache-2.0.txt`).\n\
         They are copied by `cargo xtask wasm-package`, not edited.\n",
    )
    .map_err(|error| format!("writing samples/NOTICE.md: {error}"))?;
    run("node", &["--no-warnings", "crates/pdf2-wasm/check.mjs"])
}

/// The ledger's contract with the plan: every roadmap item has a row that names
/// it, every item number a row names exists, and every *shipped* row carries
/// evidence. See `docs/FEATURES.md`.
fn ledger() -> Result<(), String> {
    let ledger = std::fs::read_to_string("docs/FEATURES.md")
        .map_err(|error| format!("docs/FEATURES.md: {error}"))?;
    let roadmap = std::fs::read_to_string("docs/ROADMAP-NEXT.md")
        .map_err(|error| format!("docs/ROADMAP-NEXT.md: {error}"))?;
    let items = ledger::roadmap_items(&roadmap);
    let rows = ledger::rows(&ledger);
    let problems = ledger::problems(&rows, &items);
    if problems.is_empty() {
        println!(
            "ledger: {} rows, {} roadmap items, every item owned and every shipped row evidenced",
            rows.len(),
            items.len()
        );
        return Ok(());
    }
    for problem in &problems {
        eprintln!("ledger: {problem}");
    }
    Err(format!("{} ledger problem(s)", problems.len()))
}

/// Parsing and checks for the feature ledger, kept apart so they can be tested.
mod ledger {
    /// One table row: its first cell, its status, and the rest of the row.
    #[derive(Debug, Clone, PartialEq)]
    pub struct Row {
        pub name: String,
        pub status: String,
        pub evidence: String,
        pub line: usize,
    }

    /// The roadmap's ranks, in the order the table lists them.
    pub fn roadmap_items(roadmap: &str) -> Vec<String> {
        roadmap
            .lines()
            .filter_map(|line| {
                let rest = line.strip_prefix("| ")?;
                let rank = rest.split(' ').next()?;
                let numeric = rank.trim_end_matches(|c: char| c.is_ascii_alphabetic());
                if !numeric.is_empty() && numeric.bytes().all(|b| b.is_ascii_digit()) {
                    Some(rank.to_owned())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Data rows of every table, with the header and separator rows dropped.
    pub fn rows(ledger: &str) -> Vec<Row> {
        let mut rows = Vec::new();
        for (index, line) in ledger.lines().enumerate() {
            let Some(rest) = line.strip_prefix("| ") else {
                continue;
            };
            if rest.starts_with("---") {
                continue;
            }
            let cells: Vec<&str> = rest.split('|').map(str::trim).collect();
            if cells.len() < 2 {
                continue;
            }
            // Header rows are the ones whose first cell is a word like "Feature",
            // "Clause" or "Fact"; they never carry a status.
            let name = cells[0].to_owned();
            let status = cells[1].to_owned();
            if !is_status(&status) {
                continue;
            }
            let evidence = cells
                .get(..cells.len().saturating_sub(1))
                .map(|cells| cells[2..].join(" | "))
                .unwrap_or_default();
            rows.push(Row {
                name,
                status,
                evidence,
                line: index + 1,
            });
        }
        rows
    }

    fn is_status(cell: &str) -> bool {
        cell.starts_with("shipped")
            || cell.starts_with("scheduled")
            || cell.starts_with("unplanned")
            || cell.starts_with("refused")
            || cell.starts_with("not applicable")
    }

    /// The item numbers a row names, from `scheduled N` or `Owner: item N`.
    fn named_items(row: &Row) -> Vec<String> {
        let mut items = Vec::new();
        if let Some(rest) = row.status.strip_prefix("scheduled ") {
            items.push(rest.split_whitespace().next().unwrap_or("").to_owned());
        }
        let mut rest = row.evidence.as_str();
        while let Some(at) = rest.find("item ") {
            let after = &rest[at + 5..];
            let token = after.split_whitespace().next().unwrap_or("");
            let numeric =
                token.trim_end_matches(|c: char| !c.is_ascii_digit() && !c.is_ascii_alphabetic());
            if !numeric.is_empty()
                && numeric.bytes().next().is_some_and(|b| b.is_ascii_digit())
                && numeric.len() <= 3
            {
                items.push(numeric.to_owned());
            }
            rest = &rest[at + 5..];
            if rest.len() < 2 {
                break;
            }
        }
        items.retain(|item| !item.is_empty());
        items
    }

    /// Everything wrong with the ledger against the plan.
    pub fn problems(rows: &[Row], items: &[String]) -> Vec<String> {
        let mut problems = Vec::new();
        for row in rows {
            if row.status.starts_with("shipped") && row.evidence.trim().is_empty() {
                problems.push(format!(
                    "line {}: `{}` is shipped but has no evidence cell",
                    row.line, row.name
                ));
            }
            for item in named_items(row) {
                if !items.contains(&item) {
                    problems.push(format!(
                        "line {}: `{}` names roadmap item {item}, which does not exist",
                        row.line, row.name
                    ));
                }
            }
        }
        for item in items {
            let owned = rows
                .iter()
                .any(|row| named_items(row).iter().any(|named| named == item));
            if !owned {
                problems.push(format!("roadmap item {item} has no row in the ledger"));
            }
        }
        problems
    }

    #[cfg(test)]
    mod tests {
        use super::{problems, roadmap_items, rows};

        const ROADMAP: &str = "\
| Rank | Item |
| --- | --- |
| 1 | oracle |
| 2b | quality |
";

        const GOOD: &str = "\
| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Oracle | shipped | `crates/pdf2-oracle`, owner: item 1 |
| Quality | scheduled 2b | `src/content.rs` |
";

        #[test]
        fn the_roadmap_ranks_are_read_in_order() {
            assert_eq!(roadmap_items(ROADMAP), vec!["1", "2b"]);
        }

        #[test]
        fn a_consistent_ledger_has_no_problems() {
            let rows = rows(GOOD);
            assert_eq!(rows.len(), 2, "{rows:?}");
            assert!(problems(&rows, &roadmap_items(ROADMAP)).is_empty());
        }

        #[test]
        fn a_dangling_item_number_is_reported() {
            let drifted = "\
| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Oracle | shipped | `crates/pdf2-oracle`, owner: item 1 |
| Quality | scheduled 2b | `src/content.rs` |
| Ghost | scheduled 99 | nothing |
";
            let problems = problems(&rows(drifted), &roadmap_items(ROADMAP));
            assert!(
                problems.iter().any(|problem| problem.contains("item 99")),
                "{problems:?}"
            );
        }

        #[test]
        fn an_unowned_roadmap_item_is_reported() {
            let missing = "\
| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Oracle | shipped | `crates/pdf2-oracle`, owner: item 1 |
";
            let problems = problems(&rows(missing), &roadmap_items(ROADMAP));
            assert!(
                problems.iter().any(|problem| problem.contains("item 2b")),
                "{problems:?}"
            );
        }

        #[test]
        fn a_shipped_row_without_evidence_is_reported() {
            let bare = "\
| Feature | Status | Evidence / owner |
| --- | --- | --- |
| Oracle | shipped |  |
| Quality | scheduled 2b | `src/content.rs` |
";
            let problems = problems(&rows(bare), &roadmap_items(ROADMAP));
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("no evidence")),
                "{problems:?}"
            );
        }
    }
}

/// Measure a Tier B tree: every PDF under the given directories, one `check`
/// per file, and a histogram of what failed and why.
///
/// This is a measurement, not a gate: the point is a published distribution over
/// hundreds of real files, so the numbers have somewhere to come from that is not
/// this repository's own fixtures. Classes are normalised — offsets and paths
/// stripped — so that "malformed cross-reference generation" is one bar rather
/// than one bar per byte position.
fn tier_b(roots: &[String]) -> Result<(), String> {
    if roots.is_empty() {
        return Err("usage: cargo xtask tier-b <dir>... [--limit N]".to_owned());
    }
    let mut limit = usize::MAX;
    let mut directories: Vec<&String> = Vec::new();
    let mut index = 0;
    while index < roots.len() {
        if roots[index] == "--limit" {
            limit = roots
                .get(index + 1)
                .and_then(|value| value.parse().ok())
                .ok_or_else(|| "--limit needs a number".to_owned())?;
            index += 2;
            continue;
        }
        directories.push(&roots[index]);
        index += 1;
    }

    let mut files: Vec<std::path::PathBuf> = Vec::new();
    for root in &directories {
        collect_pdfs(std::path::Path::new(root), &mut files)?;
    }
    files.sort();
    files.truncate(limit);
    if files.is_empty() {
        return Err("no PDFs found under the given directories".to_owned());
    }
    run("cargo", &["build", "-q", "-p", "pdf2-cli"])?;

    let mut classes: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut examples: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut readable = 0;
    for path in &files {
        let output = std::process::Command::new("target/debug/pdf2")
            .arg("check")
            .arg(path)
            .output()
            .map_err(|error| format!("pdf2 check: {error}"))?;
        let success = output.status.success();
        // The per-page cause is on stdout and the summary on stderr, so both are
        // read: the histogram wants the cause.
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let class = if success {
            readable += 1;
            "readable".to_owned()
        } else {
            normalise_failure(&text)
        };
        examples
            .entry(class.clone())
            .or_insert_with(|| path.to_string_lossy().into_owned());
        *classes.entry(class).or_insert(0) += 1;
    }

    let mut ranked: Vec<(String, usize)> = classes.into_iter().collect();
    ranked.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
    let total = files.len();
    let mut report = String::new();
    report.push_str("# Tier B corpus report\n\n");
    report.push_str(&format!(
        "{} files, {} readable ({:.1}%), {} failing.\n\n",
        total,
        readable,
        100.0 * readable as f64 / total as f64,
        total - readable
    ));
    report.push_str("| Files | Share | Class | Example |\n| --- | --- | --- | --- |\n");
    for (class, count) in &ranked {
        report.push_str(&format!(
            "| {count} | {:.1}% | {} | {} |\n",
            100.0 * *count as f64 / total as f64,
            class,
            examples.get(class).map_or("", String::as_str)
        ));
    }
    std::fs::write("tier-b-report.md", &report).map_err(|error| error.to_string())?;
    println!("{report}");
    println!("wrote tier-b-report.md");
    Ok(())
}

/// Every `.pdf` below `root`, skipping anything that is not a directory entry.
fn collect_pdfs(root: &std::path::Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !root.exists() {
        return Err(format!("{}: no such directory", root.display()));
    }
    let mut stack = vec![root.to_path_buf()];
    while let Some(directory) = stack.pop() {
        let entries = std::fs::read_dir(&directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
            {
                out.push(path);
            }
        }
    }
    Ok(())
}

/// A failure message with its coordinates removed, so equal causes rank together.
fn normalise_failure(message: &str) -> String {
    // `check` prints one `page N: text: …` line per unreadable page and a summary
    // `error:` line at the end; the page line carries the cause.
    let line = message
        .lines()
        .find(|line| line.contains(": text: ") || line.contains(": content: "))
        .or_else(|| message.lines().find(|line| line.contains("error:")))
        .or_else(|| message.lines().next())
        .unwrap_or("")
        .trim();
    if let Some(at) = line.find(": text: ").or_else(|| line.find(": content: ")) {
        let marker = if line[at..].starts_with(": text: ") {
            ": text: "
        } else {
            ": content: "
        };
        return line[at + marker.len()..].trim().to_owned();
    }
    let line = line.strip_prefix("error: ").unwrap_or(line);
    // Drop the file path prefix and any " at offset N" or "at line N".
    let mut text = match line.find(':') {
        Some(at) if !line[..at].contains(' ') => line[at + 1..].trim().to_owned(),
        _ => line.to_owned(),
    };
    while let Some(at) = text.find(" at offset ") {
        let rest = &text[at + " at offset ".len()..];
        let end = rest
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(rest.len());
        text.replace_range(at..at + " at offset ".len() + end, "");
    }
    text
}

/// Measure what the library does with the committed corpus: one line per file,
/// the classes of failure, and a reminder that this is evidence, not a gate.
fn corpus() -> Result<(), String> {
    let directory = std::path::Path::new("corpus/tier-a");
    if !directory.is_dir() {
        return Err("corpus/tier-a is missing; see docs/PROOF.md".to_owned());
    }
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| path.extension().is_some_and(|extension| extension == "pdf"))
        .collect();
    files.sort();

    run(
        "cargo",
        &["build", "-q", "-p", "pdf2-cli", "-p", "pdf2-render"],
    )?;
    let mut readable = 0;
    let mut rendered = 0;
    let mut classes: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for path in &files {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let check = std::process::Command::new("target/debug/pdf2")
            .args(["check"])
            .arg(path)
            .output()
            .map_err(|error| format!("pdf2 check: {error}"))?;
        let text = std::process::Command::new("target/debug/pdf2")
            .args(["text"])
            .arg(path)
            .output()
            .map_err(|error| format!("pdf2 text: {error}"))?;
        let render = std::process::Command::new("target/debug/pdf2-render-golden")
            .args(["--fixture"])
            .arg(path)
            .args([
                "--page",
                "1",
                "--scale",
                "1",
                "--bless",
                "--out",
                "/tmp/pdf2-corpus.png",
            ])
            .output()
            .map_err(|error| format!("pdf2-render-golden: {error}"))?;

        let mut reasons = Vec::new();
        if !text.status.success() {
            let message = format!(
                "{}{}",
                String::from_utf8_lossy(&text.stderr),
                String::from_utf8_lossy(&text.stdout)
            );
            let reason = message
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| {
                    // `error: <path>: <reason>` -> `<reason>`
                    line.rsplit(": ").next().unwrap_or(line).trim().to_owned()
                })
                .unwrap_or_else(|| "text extraction failed".to_owned());
            if !reasons.contains(&reason) {
                *classes.entry(reason.clone()).or_insert(0) += 1;
            }
            reasons.push(reason);
        }
        if !check.status.success() && text.status.success() {
            *classes.entry("check failed".to_owned()).or_insert(0) += 1;
            reasons.push("check failed".to_owned());
        }
        if render.status.success() {
            rendered += 1;
        } else {
            let message = String::from_utf8_lossy(&render.stderr);
            let reason = message
                .lines()
                .find(|line| !line.trim().is_empty())
                .map(|line| line.rsplit(": ").next().unwrap_or(line).trim().to_owned())
                .unwrap_or_else(|| "render failed".to_owned());
            if !reasons.contains(&reason) {
                *classes.entry(reason.clone()).or_insert(0) += 1;
            }
            reasons.push(reason);
        }
        if check.status.success() && text.status.success() {
            readable += 1;
        }
        println!(
            "{name:44} readable={:<5} rendered={:<5} {}",
            check.status.success(),
            render.status.success(),
            reasons.join("; ")
        );
    }
    println!(
        "\n{} files: {readable} fully readable, {rendered} rendered",
        files.len()
    );
    for (reason, count) in &classes {
        println!("  {count:>2} x {reason}");
    }
    Ok(())
}

/// The demo is a page, so it is tested the way a person uses it: in a browser.
fn demo() -> Result<(), String> {
    wasm_package()?;
    run("node", &["--no-warnings", "crates/pdf2-wasm/demo-test.mjs"])
}

fn copy(from: &str, to: &str) -> Result<(), String> {
    if let Some(parent) = std::path::Path::new(to).parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    std::fs::copy(from, to)
        .map(|_| ())
        .map_err(|error| format!("copying {from} to {to}: {error}"))
}

fn bench() -> Result<(), String> {
    run(
        "cargo",
        &["run", "--release", "-p", "pdf2-bench", "--", "--check"],
    )
}

fn fixtures() -> Result<(), String> {
    run("python3", &["tests/fixtures/make_fixtures.py"])
}

/// Rebuild the generated CMap data the library ships.
///
/// The inputs are Adobe's CMap resources (BSD-3-Clause, notice in
/// `vendor/adobe-cmaps/LICENSE.md`). Two files are enough for the one predefined
/// CMap the corpus needs: `90ms-RKSJ-H` maps Shift-JIS bytes to Adobe-Japan1
/// CIDs, and `UniJIS-UCS2-H` maps UTF-16BE code units to those same CIDs, so
/// composing it backwards gives each code the text a reader would extract.
fn cmaps() -> Result<(), String> {
    let directory = std::path::Path::new("vendor/adobe-cmaps");
    let encoding = std::fs::read(directory.join("90ms-RKSJ-H"))
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    let unicode = std::fs::read(directory.join("UniJIS-UCS2-H"))
        .map_err(|error| format!("{}: {error}", directory.display()))?;

    let codes = parse_cid_cmap(&encoding);
    let back = parse_cid_cmap(&unicode);
    if codes.is_empty() || back.is_empty() {
        return Err("a CMap parsed to no ranges".to_owned());
    }
    // Unicode (UTF-16BE code unit) for a CID. Several code units share a CID —
    // `UniJIS-UCS2-H` maps both U+65E5 (日) and U+2F47 (the Kangxi radical that
    // looks the same) to it — so a compatibility or radical code is ranked below
    // the ordinary one, and the smallest code breaks a remaining tie. Taking the
    // first match in file order picks the radical, which is how this table first
    // read 日 as ⽇.
    fn rank(code: u32) -> u8 {
        match code {
            0x2E80..=0x2FDF | 0xF900..=0xFAFF | 0xFE30..=0xFE4F => 1,
            _ => 0,
        }
    }
    let mut cid_to_unicode: std::collections::BTreeMap<u32, u32> =
        std::collections::BTreeMap::new();
    for (code, cid) in &back {
        match cid_to_unicode.get(cid) {
            Some(existing) if rank(*existing) < rank(*code) => {}
            Some(existing) if rank(*existing) == rank(*code) && *existing <= *code => {}
            _ => {
                cid_to_unicode.insert(*cid, *code);
            }
        }
    }
    let mut pairs: Vec<(u16, u16)> = Vec::new();
    for (code, cid) in &codes {
        if *code > u32::from(u16::MAX) {
            continue;
        }
        let Some(unicode) = cid_to_unicode.get(cid) else {
            continue;
        };
        let Ok(unicode) = u16::try_from(*unicode) else {
            continue;
        };
        pairs.push((
            u16::try_from(*code).map_err(|error| error.to_string())?,
            unicode,
        ));
    }
    // `90ms-RKSJ-H` maps 0x20–0x7D to Adobe-Japan1 CIDs 231–324, the
    // proportional Latin set, which runs in ASCII order; `UniJIS-UCS2-H` never
    // maps Unicode to those CIDs (it targets the Japanese ones), so the ASCII
    // half composes to nothing. The encoding's own layout says what they are.
    let mapped: std::collections::BTreeSet<u16> = pairs.iter().map(|pair| pair.0).collect();
    for code in 0x20_u16..=0x7D {
        if !mapped.contains(&code) {
            pairs.push((code, code));
        }
    }
    pairs.sort_unstable();
    pairs.dedup_by_key(|pair| pair.0);

    // Varint deltas rather than fixed pairs: the codes ascend in steps of one or
    // two and the text ascends with them, so deltas are small and the table both
    // shrinks and compresses. Pairs cost 31 KB and barely gzip; deltas cost about
    // a third of that, which is what keeps the wasm budget.
    let mut out = Vec::with_capacity(8 + pairs.len() * 2);
    out.extend_from_slice(b"P2CV");
    out.push(1); // one-byte codes
    out.push(2); // two-byte codes
    out.extend_from_slice(
        &u16::try_from(pairs.len())
            .map_err(|error| error.to_string())?
            .to_be_bytes(),
    );
    let (mut previous_code, mut previous_unicode) = (0_u32, 0_u32);
    for (code, unicode) in &pairs {
        let code = u32::from(*code);
        let unicode = u32::from(*unicode);
        push_varint(&mut out, code - previous_code);
        // Zig-zag: the text can step backwards by one or two.
        let delta = i64::from(unicode) - i64::from(previous_unicode);
        let zigzag = ((delta << 1) ^ (delta >> 63)) as u64;
        push_varint(
            &mut out,
            u32::try_from(zigzag).map_err(|error| error.to_string())?,
        );
        previous_code = code;
        previous_unicode = unicode;
    }
    // `UniJIS-UCS2-H` is a *Unicode* CMap: a two-byte code stands for the
    // character with that value, so the text table is its coverage. That is what a
    // font declaring `/Encoding /UniJIS-UCS2-H` needs to extract, and it costs one
    // table of codes rather than a second mapping.
    let mut covered: Vec<u32> = back.iter().map(|(code, _)| *code).collect();
    covered.sort_unstable();
    covered.dedup();
    // Ranges, not codes: the coverage is contiguous (a few dozen runs cover every
    // code), and 9 772 identity entries would cost ~20 KB of wasm to say "text ==
    // code". `P2VR`: widths, count, then (start delta, run length) per run.
    let mut runs: Vec<(u32, u32)> = Vec::new();
    for code in &covered {
        match runs.last_mut() {
            Some((start, length)) if *start + *length == *code => *length += 1,
            _ => runs.push((*code, 1)),
        }
    }
    let mut ids = Vec::with_capacity(8 + runs.len() * 4);
    ids.extend_from_slice(b"P2VR");
    ids.push(2); // two-byte codes only: UCS-2 is what the CMap's name says
    ids.extend_from_slice(
        &u16::try_from(runs.len())
            .map_err(|error| error.to_string())?
            .to_be_bytes(),
    );
    let mut previous = 0_u32;
    for (start, length) in &runs {
        push_varint(&mut ids, *start - previous);
        push_varint(&mut ids, *length);
        previous = *start;
    }
    let ids_path = std::path::Path::new("src/data/japan1_unijis_ucs2.bin");
    std::fs::create_dir_all("src/data").map_err(|error| error.to_string())?;
    std::fs::write(ids_path, &ids).map_err(|error| error.to_string())?;
    println!(
        "{}: {} codes in {} ranges, {} bytes",
        ids_path.display(),
        covered.len(),
        runs.len(),
        ids.len()
    );

    let path = std::path::Path::new("src/data/japan1_90ms_rksj.bin");
    std::fs::create_dir_all("src/data").map_err(|error| error.to_string())?;
    std::fs::write(path, &out).map_err(|error| format!("{}: {error}", path.display()))?;
    println!(
        "{}: {} mappings, {} bytes",
        path.display(),
        pairs.len(),
        out.len()
    );
    Ok(())
}

/// A little-endian base-128 varint.
fn push_varint(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if value == 0 {
            break;
        }
    }
}

/// The `(code, cid)` pairs of a CID CMap: `begincidrange` and `begincidchar`.
fn parse_cid_cmap(data: &[u8]) -> Vec<(u32, u32)> {
    let mut pairs = Vec::new();
    let mut tokens: Vec<Token> = Vec::new();
    let mut index = 0;
    while index < data.len() {
        match data[index] {
            b'%' => {
                while index < data.len() && data[index] != b'\n' {
                    index += 1;
                }
            }
            b'<' if data.get(index + 1) != Some(&b'<') => {
                index += 1;
                let mut value = 0_u32;
                let mut digits = 0;
                while index < data.len() && data[index] != b'>' {
                    let byte = data[index];
                    index += 1;
                    let digit = match byte {
                        b'0'..=b'9' => u32::from(byte - b'0'),
                        b'a'..=b'f' => u32::from(byte - b'a') + 10,
                        b'A'..=b'F' => u32::from(byte - b'A') + 10,
                        _ => continue,
                    };
                    value = (value << 4) | digit;
                    digits += 1;
                }
                index += 1;
                if digits > 0 {
                    tokens.push(Token::Hex(value));
                }
            }
            b'0'..=b'9' => {
                let start = index;
                while index < data.len() && data[index].is_ascii_digit() {
                    index += 1;
                }
                let text = std::str::from_utf8(&data[start..index]).unwrap_or("0");
                tokens.push(Token::Number(text.parse().unwrap_or(0)));
            }
            byte if byte.is_ascii_alphabetic() || byte == b'-' => {
                let start = index;
                while index < data.len()
                    && (data[index].is_ascii_alphanumeric() || data[index] == b'-')
                {
                    index += 1;
                }
                tokens.push(Token::Keyword(
                    std::str::from_utf8(&data[start..index])
                        .unwrap_or("")
                        .to_owned(),
                ));
            }
            _ => index += 1,
        }
    }

    let mut position = 0;
    while position < tokens.len() {
        match &tokens[position] {
            Token::Keyword(keyword) if keyword == "begincidrange" => {
                position += 1;
                while let (
                    Some(Token::Hex(low)),
                    Some(Token::Hex(high)),
                    Some(Token::Number(cid)),
                ) = (
                    tokens.get(position),
                    tokens.get(position + 1),
                    tokens.get(position + 2),
                ) {
                    for code in *low..=*high {
                        pairs.push((code, *cid + (code - *low)));
                    }
                    position += 3;
                }
            }
            Token::Keyword(keyword) if keyword == "begincidchar" => {
                position += 1;
                while let (Some(Token::Hex(code)), Some(Token::Number(cid))) =
                    (tokens.get(position), tokens.get(position + 1))
                {
                    pairs.push((*code, *cid));
                    position += 2;
                }
            }
            _ => position += 1,
        }
    }
    pairs
}

enum Token {
    Hex(u32),
    Number(u32),
    Keyword(String),
}

fn bless() -> Result<(), String> {
    let fixtures = Command::new("python3")
        .arg("tests/fixtures/make_fixtures.py")
        .status()
        .map_err(|error| format!("python3: {error}"))?;
    if !fixtures.success() {
        return Err("generating fixtures failed".to_owned());
    }
    let tests = Command::new("cargo")
        .args(["test", "-p", "pdf2-render"])
        .env("BLESS", "1")
        .status()
        .map_err(|error| format!("cargo: {error}"))?;
    if !tests.success() {
        return Err("blessing the goldens failed".to_owned());
    }
    for (fixture, golden) in [("paths", "paths.png"), ("images", "images.png")] {
        let fixture = format!("tests/fixtures/{fixture}.pdf");
        let golden = format!("tests/fixtures/golden/{golden}");
        run(
            "cargo",
            &[
                "run",
                "--quiet",
                "-p",
                "pdf2-render",
                "--bin",
                "pdf2-render-golden",
                "--",
                "--fixture",
                &fixture,
                "--out",
                &golden,
                "--bless",
            ],
        )?;
    }
    println!("re-render the diff before committing: `git diff --stat tests/fixtures/golden`");
    Ok(())
}

fn run(program: &str, args: &[&str]) -> Result<(), String> {
    run_env(program, args, &[])
}

fn run_env(program: &str, args: &[&str], env: &[(&str, &str)]) -> Result<(), String> {
    println!("\n$ {program} {}", args.join(" "));
    let mut command = Command::new(program);
    command.args(args);
    for (name, value) in env {
        command.env(name, value);
    }
    let status = command
        .status()
        .map_err(|error| format!("running `{program}`: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!("`{program} {}` failed", args.join(" ")))
}

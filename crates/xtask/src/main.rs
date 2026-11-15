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
        "bench" => bench(),
        "fixtures" => fixtures(),
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
  cargo xtask ledger    check that docs/FEATURES.md and the roadmap agree
  cargo xtask bench     measure and compare with docs/performance-baseline.json
  cargo xtask fixtures  regenerate tests/fixtures/*.pdf
  cargo xtask bless     re-render the committed golden PNGs

Exit codes: 0 all good, 1 a step failed.";

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
    for file in ["pdf2_wasm.js", "pdf2_wasm_bg.wasm"] {
        copy(
            &format!("crates/pdf2-wasm/pkg/{file}"),
            &format!("crates/pdf2-wasm/demo/pkg/{file}"),
        )?;
    }
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
    ] {
        copy(from, to)?;
    }
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

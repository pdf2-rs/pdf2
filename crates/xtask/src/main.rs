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

const GATE_STEPS: &[&str] = &["format", "clippy", "test", "features", "docs"];

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
        "no-std" => no_std(),
        "wasm" => wasm(),
        "wasm-package" => wasm_package(),
        "demo" => demo(),
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
            "tests/fixtures/encrypted-r6.pdf",
            "crates/pdf2-wasm/demo/samples/encrypted.pdf",
        ),
    ] {
        copy(from, to)?;
    }
    run("node", &["--no-warnings", "crates/pdf2-wasm/check.mjs"])
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

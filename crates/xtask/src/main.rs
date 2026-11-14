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
    let wasm = "target/wasm32-unknown-unknown/release/pdf2_wasm.wasm";
    run(
        "wasm-bindgen",
        &[
            "--target",
            "nodejs",
            "--out-dir",
            "crates/pdf2-wasm/pkg-node",
            wasm,
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
            wasm,
        ],
    )?;
    run("node", &["--no-warnings", "crates/pdf2-wasm/check.mjs"])
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

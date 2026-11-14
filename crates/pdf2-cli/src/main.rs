//! `pdf2` — command-line front end for the `pdf2` crate.
//!
//! The binary is deliberately dependency-free: argument parsing, JSON output
//! and the commands are all hand-written so that the CLI brings no third-party
//! code. Every failure becomes a message on stderr and an exit code — 1 for a
//! bad command line, 2 for a file that cannot be read, decrypted, parsed or
//! interpreted — and never a panic.

mod args;
mod commands;
mod json;

use std::process::ExitCode;

use args::{Command, Failure};

const USAGE: &str = "\
pdf2 — inspect PDF 2.0 files

Usage:
  pdf2 info <file> [--password <pw>] [--json]
  pdf2 pages <file> [--password <pw>] [--json]
  pdf2 text <file> [--page <n> | --all] [--password <pw>] [--json]
  pdf2 check <file> [--password <pw>] [--json]
  pdf2 --help
  pdf2 --version

Commands:
  info    document version, page count, encryption and metadata state
  pages   every page: object id, media box, crop box, rotation, content ids
  text    extracted text; page 1 unless --page or --all is given
  check   open, decode and extract every page, and report what fails

Options:
  --password <pw>, -p <pw>   password for an encrypted document
  --page <n>, -n <n>         page for `text`, counting from 1
  --all, -a                  every page for `text`
  --json                     machine-readable output
  --help, -h                 this help
  --version, -V              version

Exit codes:
  0  success
  1  the command line is wrong
  2  the file could not be read, decrypted, parsed or interpreted
";

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match run(&arguments) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            eprintln!("error: {}", failure.message());
            if matches!(failure, Failure::Usage(_)) {
                eprintln!("\n{USAGE}");
            }
            ExitCode::from(failure.exit_code())
        }
    }
}

fn run(arguments: &[String]) -> Result<(), Failure> {
    match args::parse(arguments)? {
        Command::Help => {
            print!("{USAGE}");
            Ok(())
        }
        Command::Version => {
            println!("pdf2-cli {}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Command::Info(run) => commands::info(&run),
        Command::Pages(run) => commands::pages(&run),
        Command::Text { run, selection } => commands::text(&run, selection),
        Command::Check(run) => commands::check(&run),
    }
}

//! Command-line parsing for the `pdf2` command.
//!
//! Parsing is hand-written so that the CLI pulls in no third-party crates, and
//! every rejection carries the exit code it stands for: a bad command line is
//! a usage error (1), anything about the file is an input error (2).

use std::path::PathBuf;

/// Why a command could not run, together with its exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The command line itself is wrong: exit code 1.
    Usage(String),
    /// The file could not be read, decrypted, parsed or interpreted: exit code 2.
    Input(String),
}

impl Failure {
    /// A bad command line.
    pub fn usage(message: impl Into<String>) -> Self {
        Self::Usage(message.into())
    }

    /// A problem with the file, its password or its contents.
    pub fn input(message: impl Into<String>) -> Self {
        Self::Input(message.into())
    }

    /// The process exit code for this failure.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 1,
            Self::Input(_) => 2,
        }
    }

    /// The message printed after `error: `.
    pub fn message(&self) -> &str {
        match self {
            Self::Usage(message) | Self::Input(message) => message,
        }
    }
}

/// Options shared by every command that opens a file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Run {
    /// The file to open.
    pub file: PathBuf,
    /// Password for an encrypted document; empty means "no password".
    pub password: Vec<u8>,
    /// Machine-readable output.
    pub json: bool,
}

/// Which pages `text` should print.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    /// One page, numbered from 1.
    Page(usize),
    /// Every page, in order.
    All,
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `--help`: print usage and exit 0.
    Help,
    /// `--version`: print the version and exit 0.
    Version,
    /// `info`: document facts.
    Info(Run),
    /// `pages`: one line per page.
    Pages(Run),
    /// `text`: extracted text.
    Text { run: Run, selection: Selection },
    /// `check`: read every page and report failures.
    Check(Run),
}

/// Parse the arguments following the program name.
pub fn parse(args: &[String]) -> Result<Command, Failure> {
    let Some(name) = args.first() else {
        return Err(Failure::usage("missing command"));
    };
    let rest = &args[1..];
    match name.as_str() {
        "--help" | "-h" | "help" => Ok(Command::Help),
        "--version" | "-V" => Ok(Command::Version),
        "info" => Ok(Command::Info(run_options(rest, false)?)),
        "pages" => Ok(Command::Pages(run_options(rest, false)?)),
        "check" => Ok(Command::Check(run_options(rest, false)?)),
        "text" => parse_text(rest),
        other => Err(Failure::usage(format!("unknown command `{other}`"))),
    }
}

/// Options for commands without page selection.
fn run_options(args: &[String], allow_selection: bool) -> Result<Run, Failure> {
    let (run, selection) = options(args, allow_selection)?;
    if selection.is_some() {
        // Unreachable: selection options are only accepted when requested.
        return Err(Failure::usage("page selection is not available here"));
    }
    Ok(run)
}

fn parse_text(args: &[String]) -> Result<Command, Failure> {
    let (run, selection) = options(args, true)?;
    Ok(Command::Text {
        run,
        selection: selection.unwrap_or(Selection::Page(1)),
    })
}

/// Shared option loop. Returns the run options and, when `allow_selection` is
/// true, the page selection the command line asked for.
fn options(args: &[String], allow_selection: bool) -> Result<(Run, Option<Selection>), Failure> {
    let mut run = Run::default();
    let mut file: Option<PathBuf> = None;
    let mut page: Option<usize> = None;
    let mut all = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--password" | "-p" => {
                run.password = value(args, index)?.as_bytes().to_vec();
                index += 2;
            }
            "--json" => {
                run.json = true;
                index += 1;
            }
            "--page" | "-n" if allow_selection => {
                let text = value(args, index)?;
                let number: usize = text.parse().map_err(|_| {
                    Failure::usage(format!("`--page` needs a number, got `{text}`"))
                })?;
                if number == 0 {
                    return Err(Failure::usage("`--page` counts from 1"));
                }
                page = Some(number);
                index += 2;
            }
            "--all" | "-a" if allow_selection => {
                all = true;
                index += 1;
            }
            option if option.starts_with('-') => {
                return Err(Failure::usage(format!("unknown option `{option}`")));
            }
            value => {
                if file.is_some() {
                    return Err(Failure::usage("only one file may be given"));
                }
                file = Some(PathBuf::from(value));
                index += 1;
            }
        }
    }
    run.file = file.ok_or_else(|| Failure::usage("missing <file>"))?;
    if all && page.is_some() {
        return Err(Failure::usage("`--page` and `--all` cannot be combined"));
    }
    let selection = if all {
        Some(Selection::All)
    } else {
        page.map(Selection::Page)
    };
    Ok((run, selection))
}

fn value(args: &[String], index: usize) -> Result<&str, Failure> {
    args.get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| Failure::usage(format!("`{}` needs a value", args[index])))
}

#[cfg(test)]
mod tests {
    use super::{Command, Failure, Run, Selection, parse};
    use std::path::PathBuf;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    fn run(file: &str) -> Run {
        Run {
            file: PathBuf::from(file),
            password: Vec::new(),
            json: false,
        }
    }

    #[test]
    fn help_and_version_are_flags() {
        assert_eq!(parse(&args(&["--help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["-h"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["help"])), Ok(Command::Help));
        assert_eq!(parse(&args(&["--version"])), Ok(Command::Version));
        assert_eq!(parse(&args(&["-V"])), Ok(Command::Version));
    }

    #[test]
    fn every_command_takes_a_file_password_and_json() {
        for name in ["info", "pages", "check"] {
            let parsed = parse(&args(&[name, "a.pdf", "--password", "pw", "--json"]));
            let Ok(command) = parsed else {
                panic!("{name} did not parse: {parsed:?}");
            };
            let expected = Run {
                file: PathBuf::from("a.pdf"),
                password: b"pw".to_vec(),
                json: true,
            };
            let actual = match command {
                Command::Info(run) | Command::Pages(run) | Command::Check(run) => run,
                other => panic!("{name} parsed as {other:?}"),
            };
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn text_defaults_to_the_first_page() {
        assert_eq!(
            parse(&args(&["text", "a.pdf"])),
            Ok(Command::Text {
                run: run("a.pdf"),
                selection: Selection::Page(1)
            })
        );
        assert_eq!(
            parse(&args(&["text", "a.pdf", "--page", "3"])),
            Ok(Command::Text {
                run: run("a.pdf"),
                selection: Selection::Page(3)
            })
        );
        assert_eq!(
            parse(&args(&["text", "a.pdf", "--all"])),
            Ok(Command::Text {
                run: run("a.pdf"),
                selection: Selection::All
            })
        );
    }

    #[test]
    fn usage_errors_are_rejected() {
        let cases: &[&[&str]] = &[
            &[],
            &["frobnicate"],
            &["info"],
            &["info", "a.pdf", "b.pdf"],
            &["info", "a.pdf", "--password"],
            &["info", "a.pdf", "--nope"],
            &["text", "a.pdf", "--page", "0"],
            &["text", "a.pdf", "--page", "x"],
            &["text", "a.pdf", "--all", "--page", "2"],
            &["info", "a.pdf", "--page", "2"],
            &["info", "a.pdf", "--all"],
        ];
        for case in cases {
            match parse(&args(case)) {
                Err(Failure::Usage(_)) => {}
                other => panic!("{case:?} parsed as {other:?}"),
            }
        }
    }

    #[test]
    fn exit_codes_distinguish_usage_from_input() {
        assert_eq!(Failure::usage("x").exit_code(), 1);
        assert_eq!(Failure::input("x").exit_code(), 2);
    }
}

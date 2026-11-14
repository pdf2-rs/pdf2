//! `pdf2-bench` — measure the `pdf2` crate and guard a committed baseline.
//!
//! Measuring is deliberately plain: medians of repeated runs for throughput,
//! deterministic allocation counts from a counting global allocator, and the
//! process's peak resident memory. `--check` compares the numbers with
//! `docs/performance-baseline.json` and fails when a metric moves past the
//! tolerance, so a regression breaks CI instead of being discovered later.
#![allow(unsafe_code)] // the counting global allocator has to call the system allocator.

mod alloc;
mod baseline;
mod corpus;
mod json;
mod metrics;

use std::path::PathBuf;
use std::process::ExitCode;

use baseline::{Baseline, DEFAULT_TOLERANCE};
use json::{number, string};
use metrics::{Metrics, DEFAULT_RUNS};

#[global_allocator]
static GLOBAL: alloc::Counting = alloc::Counting;

const USAGE: &str = "\
pdf2-bench — measure pdf2 and compare with the committed baseline

Usage:
  pdf2-bench [options]

Options:
  --runs <n>              timed repetitions per operation (default 5)
  --json                  print the measurements as JSON
  --check                 compare with the baseline; exit 1 on a regression
  --baseline <path>       baseline file (default docs/performance-baseline.json)
  --write-baseline <path> record this platform's measurements in the baseline
  --tolerance <fraction>  allowed change before a metric counts as a regression
                          (default 0.10)
  --help, -h              this help

Environment:
  PDF2_CORPUS=<dir>       also measure the PDFs in this directory; they are
                          reported but never gated

Exit codes:
  0  everything within tolerance
  1  at least one metric regressed
  2  the benchmark itself failed
";

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(2)
        }
    }
}

fn run(arguments: Vec<String>) -> Result<bool, String> {
    let options = parse(&arguments)?;
    if options.help {
        print!("{USAGE}");
        return Ok(true);
    }

    let scratch = std::env::temp_dir().join(format!("pdf2-bench-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).map_err(|error| format!("{}: {error}", scratch.display()))?;
    let groups = corpus::build(&scratch)?;
    let mut measured = metrics::collect(&groups, options.runs)?;

    // A regression gate on wall-clock time is only useful if a noisy sample
    // cannot trip it: when a metric looks broken, measure once more and judge
    // the second result.
    let mut report = None;
    if options.check {
        let baseline = Baseline::load(&options.baseline)?;
        let mut current = baseline.check(&measured, options.tolerance);
        if !current.is_ok() {
            println!(
                "{} metric(s) beyond tolerance; measuring once more to rule out noise",
                current.regressions.len()
            );
            measured = metrics::collect(&groups, options.runs)?;
            current = baseline.check(&measured, options.tolerance);
            if current.is_ok() {
                println!("within tolerance on the second measurement");
            }
        }
        report = Some((baseline, current));
    }

    let _ = std::fs::remove_dir_all(&scratch);

    if let Some(path) = &options.write_baseline {
        let mut baseline = if path.exists() {
            Baseline::load(path)?
        } else {
            Baseline::new()
        };
        baseline.generated = today();
        baseline.upsert(&measured.platform, measured.values.clone());
        baseline.save(path)?;
        println!(
            "wrote {} ({} platform(s), this one is {})",
            path.display(),
            baseline.entries.len(),
            measured.platform
        );
    }

    if options.json {
        println!("{}", metrics_json(&measured));
    } else {
        print!("{}", metrics::table(&measured));
    }

    let Some((baseline, report)) = report else {
        return Ok(true);
    };
    println!(
        "\ncompared {} metric(s) with {} (written {})",
        report.compared.len(),
        options.baseline.display(),
        if baseline.generated.is_empty() {
            "unknown"
        } else {
            &baseline.generated
        }
    );
    if !report.skipped.is_empty() {
        println!(
            "skipped {} metric(s): no gated baseline value for platform {}",
            report.skipped.len(),
            measured.platform
        );
        if options.verbose {
            for name in &report.skipped {
                println!("  skipped {name}");
            }
        }
    }
    for (name, reference, current) in &report.regressions {
        println!(
            "REGRESSION {name}: baseline {}, now {}",
            format_value(name, *reference),
            format_value(name, *current)
        );
    }
    if report.is_ok() {
        println!("within {:.0}% tolerance", options.tolerance * 100.0);
    } else {
        println!(
            "{} regression(s) beyond {:.0}% tolerance",
            report.regressions.len(),
            options.tolerance * 100.0
        );
    }
    Ok(report.is_ok())
}

fn format_value(name: &str, value: f64) -> String {
    if name.ends_with(".allocations")
        || name.ends_with(".alloc_bytes")
        || name.ends_with(".peak_rss_kib")
    {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn metrics_json(metrics: &Metrics) -> String {
    let mut out = String::from("{\n");
    out.push_str(&format!("  \"platform\": {},\n", string(&metrics.platform)));
    out.push_str("  \"values\": {\n");
    let lines: Vec<String> = metrics
        .values
        .iter()
        .map(|(name, value)| format!("    {}: {}", string(name), number(*value)))
        .collect();
    out.push_str(&lines.join(",\n"));
    out.push_str("\n  }\n}");
    out
}

/// The date recorded in the baseline: `date -u +%Y-%m-%d`, or `unknown`.
fn today() -> String {
    std::process::Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[derive(Debug, Clone)]
struct Options {
    runs: usize,
    json: bool,
    check: bool,
    verbose: bool,
    baseline: PathBuf,
    write_baseline: Option<PathBuf>,
    tolerance: f64,
    help: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            runs: DEFAULT_RUNS,
            json: false,
            check: false,
            verbose: false,
            baseline: default_baseline(),
            write_baseline: None,
            tolerance: DEFAULT_TOLERANCE,
            help: false,
        }
    }
}

fn default_baseline() -> PathBuf {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/performance-baseline.json");
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn parse(arguments: &[String]) -> Result<Options, String> {
    let mut options = Options::default();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--help" | "-h" => {
                options.help = true;
                index += 1;
            }
            "--json" => {
                options.json = true;
                index += 1;
            }
            "--check" => {
                options.check = true;
                index += 1;
            }
            "--verbose" | "-v" => {
                options.verbose = true;
                index += 1;
            }
            "--runs" => {
                let text = value(arguments, index)?;
                options.runs = text
                    .parse()
                    .map_err(|_| format!("`--runs` needs a number, got `{text}`"))?;
                if options.runs == 0 {
                    return Err("`--runs` must be at least 1".to_owned());
                }
                index += 2;
            }
            "--tolerance" => {
                let text = value(arguments, index)?;
                options.tolerance = text
                    .parse()
                    .map_err(|_| format!("`--tolerance` needs a fraction, got `{text}`"))?;
                if !(0.0..1.0).contains(&options.tolerance) {
                    return Err("`--tolerance` must be between 0 and 1".to_owned());
                }
                index += 2;
            }
            "--baseline" => {
                options.baseline = PathBuf::from(value(arguments, index)?);
                index += 2;
            }
            "--write-baseline" => {
                options.write_baseline = Some(PathBuf::from(value(arguments, index)?));
                index += 2;
            }
            other => return Err(format!("unknown option `{other}`")),
        }
    }
    Ok(options)
}

fn value(arguments: &[String], index: usize) -> Result<&str, String> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| format!("`{}` needs a value", arguments[index]))
}

#[cfg(test)]
mod tests {
    use super::{metrics_json, parse};
    use crate::json;
    use crate::metrics::Metrics;
    use std::collections::BTreeMap;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn defaults_are_sane() {
        let options = parse(&args(&[])).expect("parsed");
        assert!(!options.check);
        assert!(!options.json);
        assert!(options.baseline.ends_with("docs/performance-baseline.json"));
        assert_eq!(options.tolerance, 0.10);
    }

    #[test]
    fn options_parse() {
        let options = parse(&args(&[
            "--check",
            "--json",
            "--runs",
            "3",
            "--tolerance",
            "0.25",
            "--baseline",
            "b.json",
            "--write-baseline",
            "w.json",
        ]))
        .expect("parsed");
        assert!(options.check);
        assert!(options.json);
        assert_eq!(options.runs, 3);
        assert_eq!(options.tolerance, 0.25);
        assert_eq!(options.baseline.to_str(), Some("b.json"));
        assert_eq!(
            options
                .write_baseline
                .as_deref()
                .and_then(|path| path.to_str()),
            Some("w.json")
        );
    }

    #[test]
    fn bad_options_are_errors() {
        for arguments in [
            vec!["--nope"],
            vec!["--runs"],
            vec!["--runs", "0"],
            vec!["--runs", "x"],
            vec!["--tolerance", "2"],
            vec!["--tolerance", "x"],
        ] {
            assert!(parse(&args(&arguments)).is_err(), "{arguments:?}");
        }
    }

    #[test]
    fn measurements_serialise_as_json() {
        let mut values = BTreeMap::new();
        values.insert("fixtures.read_mb_s".to_owned(), 12.5);
        values.insert("fixtures.allocations".to_owned(), 900.0);
        let text = metrics_json(&Metrics {
            platform: "test-platform".to_owned(),
            values,
        });
        let parsed = json::parse(&text).expect("valid json");
        assert_eq!(
            parsed.get("platform").and_then(json::Json::as_str),
            Some("test-platform")
        );
        let metrics = parsed.get("values").expect("values");
        assert_eq!(
            metrics
                .get("fixtures.read_mb_s")
                .and_then(json::Json::as_f64),
            Some(12.5)
        );
    }
}

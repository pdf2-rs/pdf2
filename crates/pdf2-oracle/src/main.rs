//! Compare this renderer with independent engines and gate on the measured
//! tolerance, as `docs/PROOF.md` describes.
//!
//! ```text
//! # check against the committed tolerances (what CI runs)
//! pdf2-oracle --cases corpus/oracle-cases.txt --tolerances corpus/tolerances.tsv
//!
//! # measure the inter-oracle spread for each case and rewrite the tolerances
//! pdf2-oracle --cases corpus/oracle-cases.txt --calibrate \
//!     --write-tolerances corpus/tolerances.tsv
//! ```
//!
//! Each case is also timed — this renderer's own wall clock for that page — and
//! compared with the committed budget in `corpus/render-budget.tsv` (item 10c).
//! A regression in time is a failure like any other: the budget is a committed
//! number with the machine it was measured on written beside it, not a report.
//!
//! The oracles are poppler (`pdftoppm`), mupdf (`mutool draw`) and Ghostscript
//! (`gs`); each is used if present, and at least two are required to measure a
//! spread at all. Everything is compared at a downsampled scale (`--blocks`,
//! default 4): this renderer has no anti-aliasing, and comparing hard edges with
//! anti-aliased ones pixel by pixel would measure the rasteriser's model, not
//! its correctness.

mod ppm;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use pdf2::Document;
use pdf2_render::{RenderOptions, render_page_number};
use ppm::{Distance, Ppm, distance, downsample};

/// One page to compare.
struct Case {
    name: String,
    path: PathBuf,
    page: usize,
}

struct Options {
    cases: PathBuf,
    tolerances: PathBuf,
    budget: Option<PathBuf>,
    dpi: f64,
    blocks: u32,
    calibrate: bool,
    write_tolerances: Option<PathBuf>,
    summary: Option<PathBuf>,
    /// Per-case mean-distance ceilings that apply at any block size: the
    /// anti-aliasing gate (`corpus/aa-ceilings.tsv`).
    aa_ceilings: Option<PathBuf>,
}

const FLOOR_OVER: f64 = 0.01;
const FLOOR_MEAN: f64 = 2.0;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), String> {
    let options = parse_args(std::env::args().skip(1).collect())?;
    let cases = read_cases(&options.cases)?;
    if cases.is_empty() {
        return Err(format!("{} lists no cases", options.cases.display()));
    }
    let oracles = Oracles::detect();
    if oracles.len() < 2 {
        return Err(format!(
            "need at least two oracles to measure a spread; found {:?}",
            oracles.names()
        ));
    }
    let known = read_tolerances(&options.tolerances).unwrap_or_default();
    // A missing budget is a warning, not a failure: the first run of a new
    // machine has nothing to compare with yet, and refusing to measure would be
    // refusing to produce the number the budget is set from.
    let ceilings = match &options.aa_ceilings {
        Some(path) if path.exists() => read_ceilings(path)?,
        Some(path) => {
            eprintln!(
                "warning: {} not found; no per-case ceiling is applied",
                path.display()
            );
            std::collections::BTreeMap::new()
        }
        None => std::collections::BTreeMap::new(),
    };
    let budget = match &options.budget {
        Some(path) if path.exists() => read_budget(path)?,
        Some(path) => {
            eprintln!(
                "warning: {} not found; timing is measured but not gated",
                path.display()
            );
            std::collections::BTreeMap::new()
        }
        None => std::collections::BTreeMap::new(),
    };
    let scale = options.dpi / 72.0;

    println!(
        "{:28} {:>8} {:>8} {:>8} {:>8} {:>8} {:>9} verdict",
        "case", "ours/1", "ours/2", "spread", "allowed", "blocks", "ms"
    );
    let mut failures = 0;
    let mut calibrated = Vec::new();
    // Every case's inter-oracle spread, whether or not `--calibrate` is on: the
    // summary CSV is where a per-case mean is read from, and it used to be written
    // from `calibrated`, which only `--calibrate` fills — so `--summary` on its own
    // produced a header and no rows.
    let mut measured = Vec::new();
    let mut timings: Vec<(String, f64)> = Vec::new();
    let mut our_distances: Vec<(String, Distance)> = Vec::new();
    for case in &cases {
        let bytes = std::fs::read(&case.path)
            .map_err(|error| format!("{}: {error}", case.path.display()))?;
        let document = Document::open(bytes.clone())
            .map_err(|error| format!("{}: {error}", case.path.display()))?;
        // The case list is 1-based, like a reader's page number; the renderer
        // takes an index.
        // The timed region is the render alone: parsing is measured by the
        // benchmark suite, and mixing the two would make a fast render look slow
        // because a document happened to be large.
        let started = std::time::Instant::now();
        let ours = render_ours(&document, case.page.saturating_sub(1), scale)
            .map_err(|error| format!("{}: {error}", case.name))?;
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        let mut oracle_images = Vec::new();
        for oracle in &oracles.list {
            match oracle.render(&case.path, case.page, options.dpi) {
                Ok(image) => oracle_images.push((oracle.name, image)),
                // An engine that cannot open the file is not a comparison.
                Err(error) => eprintln!(
                    "warning: {} skipped for {}: {error}",
                    oracle.name, case.name
                ),
            }
        }
        if oracle_images.len() < 2 {
            return Err(format!(
                "{}: fewer than two oracles rendered this case",
                case.name
            ));
        }
        let ours_small = downsample(&ours, options.blocks);
        let oracle_small: Vec<(&str, Ppm)> = oracle_images
            .iter()
            .map(|(name, image)| (*name, downsample(image, options.blocks)))
            .collect();

        let mut ours_distances = Vec::new();
        for (_, image) in &oracle_small {
            ours_distances.push(distance(&ours_small, image)?);
        }
        let mut spread = Distance {
            mean: 0.0,
            over: 0.0,
        };
        for left in 0..oracle_small.len() {
            for right in left + 1..oracle_small.len() {
                let between = distance(&oracle_small[left].1, &oracle_small[right].1)?;
                spread.mean = spread.mean.max(between.mean);
                spread.over = spread.over.max(between.over);
            }
        }

        let median = median_distance(&ours_distances);
        let allowed = Distance {
            mean: (spread.mean * 1.1).max(FLOOR_MEAN),
            over: (spread.over * 1.1).max(FLOOR_OVER),
        };
        let over_time = !options.calibrate
            && matches!(budget.get(&case.name), Some(limit) if elapsed_ms > *limit);
        let within = median.over <= allowed.over && median.mean <= allowed.mean;
        // A committed per-case ceiling is what makes anti-aliasing gated rather
        // than merely present: the AA-less renderer missed `book#1`'s 3.2 by
        // half a channel, and the number comes from both measurements.
        let coarse = !options.calibrate
            && ceilings
                .get(&case.name)
                .is_some_and(|ceiling| median.mean > *ceiling);
        let verdict = if options.calibrate {
            "calibrated"
        } else if !within {
            failures += 1;
            "OVER"
        } else if coarse {
            failures += 1;
            "COARSE"
        } else if over_time {
            failures += 1;
            "SLOW"
        } else {
            "ok"
        };
        println!(
            "{:28} {:>8.4} {:>8.4} {:>8.4} {:>8.4} {:>8} {:>9.1} {}",
            case.name,
            ours_distances.first().map_or(0.0, |d| d.over),
            ours_distances.get(1).map_or(0.0, |d| d.over),
            spread.over,
            allowed.over,
            options.blocks,
            elapsed_ms,
            verdict
        );
        if coarse {
            println!(
                "  mean distance {:.3} is over the committed {:.3} for this case \
                 (corpus/aa-ceilings.tsv); anti-aliasing regressed or was skipped",
                median.mean,
                ceilings.get(&case.name).copied().unwrap_or(0.0)
            );
        }
        if over_time {
            println!(
                "  render took {:.1} ms against a {:.0} ms budget; raise it in \
                 corpus/render-budget.tsv deliberately, with a reason",
                elapsed_ms,
                budget.get(&case.name).copied().unwrap_or(0.0)
            );
        }
        timings.push((case.name.clone(), elapsed_ms));
        measured.push((case.name.clone(), spread));
        our_distances.push((case.name.clone(), median));
        if options.calibrate {
            calibrated.push((case.name.clone(), spread));
        } else if verdict == "OVER" {
            println!(
                "  over by {:+.4} pixels ({:.4} vs {:.4}) and {:+.3} mean ({:.3} vs {:.3}); oracles {:?}",
                median.over - allowed.over,
                median.over,
                allowed.over,
                median.mean - allowed.mean,
                median.mean,
                allowed.mean,
                oracles.names()
            );
            if let Some(known) = known.get(&case.name) {
                println!(
                    "  committed tolerance: {:.4} pixels, {:.3} mean",
                    known.over, known.mean
                );
            }
        }
    }

    if let Some(path) = &options.write_tolerances {
        write_tolerances(path, &calibrated)?;
        println!("wrote {} case(s) to {}", calibrated.len(), path.display());
    }
    if let Some(path) = &options.summary {
        // `ours_*` is this renderer's distance to the oracles at whatever
        // `--blocks` was asked for: the 4×4 gate's numbers, or the 1×1 numbers
        // that anti-aliasing needs (`docs/RENDERING-QUALITY.md`).
        let mut text = String::from("case,spread_over,spread_mean,ours_over,ours_mean,render_ms\n");
        for (name, spread) in &measured {
            let ms = timings
                .iter()
                .find(|(case, _)| case == name)
                .map_or(0.0, |(_, ms)| *ms);
            let distance = our_distances
                .iter()
                .find(|(case, _)| case == name)
                .map(|(_, d)| *d);
            text.push_str(&format!(
                "{name},{:.6},{:.6},{:.6},{:.6},{ms:.1}\n",
                spread.over,
                spread.mean,
                distance.map_or(0.0, |d| d.over),
                distance.map_or(0.0, |d| d.mean)
            ));
        }
        std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    if options.calibrate {
        return Ok(());
    }
    if failures > 0 {
        return Err(format!("{failures} case(s) outside their tolerance"));
    }
    println!("all {} case(s) within tolerance", cases.len());
    Ok(())
}

fn parse_args(args: Vec<String>) -> Result<Options, String> {
    let mut options = Options {
        cases: PathBuf::from("corpus/oracle-cases.txt"),
        budget: Some(PathBuf::from("corpus/render-budget.tsv")),
        tolerances: PathBuf::from("corpus/tolerances.tsv"),
        dpi: 144.0,
        blocks: 4,
        calibrate: false,
        write_tolerances: None,
        summary: None,
        aa_ceilings: None,
    };
    let mut index = 0;
    while index < args.len() {
        let value = |index: usize| -> Result<&String, String> {
            args.get(index + 1)
                .ok_or_else(|| format!("{} needs a value", args[index]))
        };
        match args[index].as_str() {
            "--cases" => {
                options.cases = PathBuf::from(value(index)?);
                index += 2;
            }
            "--tolerances" => {
                options.tolerances = PathBuf::from(value(index)?);
                index += 2;
            }
            "--dpi" => {
                options.dpi = value(index)?.parse().map_err(|_| "bad --dpi".to_owned())?;
                index += 2;
            }
            "--blocks" => {
                options.blocks = value(index)?
                    .parse()
                    .map_err(|_| "bad --blocks".to_owned())?;
                index += 2;
            }
            "--calibrate" => {
                options.calibrate = true;
                index += 1;
            }
            "--write-tolerances" => {
                options.write_tolerances = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            "--budget" => {
                options.budget = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            "--summary" => {
                options.summary = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            "--aa-ceilings" => {
                options.aa_ceilings = Some(PathBuf::from(value(index)?));
                index += 2;
            }
            "--help" | "-h" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
        }
    }
    Ok(options)
}

const USAGE: &str = "\
pdf2-oracle — compare this renderer with independent engines

Usage:
  pdf2-oracle --cases <file> [options]

Options:
  --cases <path>            list of `path [page]` lines (default corpus/oracle-cases.txt)
  --tolerances <path>       committed tolerances (default corpus/tolerances.tsv)
  --dpi <number>            render resolution (default 144)
  --blocks <n>              downsample blocks before comparing (default 4)
  --calibrate               measure the inter-oracle spread instead of gating
  --write-tolerances <path> write the measured spread to this file
  --budget <path>           per-page render budget (default corpus/render-budget.tsv)
  --summary <path>          write the measured spread and timing as CSV
  --aa-ceilings <path>      per-case mean-distance ceilings (`case<TAB>mean`)
  --help, -h                this help";

/// The committed per-page render budgets: `case<TAB>milliseconds`.
/// The committed per-case ceilings: `case<TAB>mean distance`.
fn read_ceilings(path: &Path) -> Result<std::collections::BTreeMap<String, f64>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut ceilings = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('\t') else {
            return Err(format!(
                "{}: `{line}` is not `case<TAB>mean`",
                path.display()
            ));
        };
        let value: f64 = value
            .trim()
            .parse()
            .map_err(|_| format!("{}: `{value}` is not a number", path.display()))?;
        ceilings.insert(name.trim().to_owned(), value);
    }
    Ok(ceilings)
}

fn read_budget(path: &Path) -> Result<std::collections::BTreeMap<String, f64>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut budget = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, value)) = line.split_once('\t') else {
            return Err(format!("{}: `{line}` is not `case<TAB>ms`", path.display()));
        };
        let value: f64 = value
            .trim()
            .parse()
            .map_err(|_| format!("{}: `{value}` is not a number", path.display()))?;
        budget.insert(name.trim().to_owned(), value);
    }
    Ok(budget)
}

fn read_cases(path: &Path) -> Result<Vec<Case>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut cases = Vec::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let file = parts.next().expect("non-empty");
        let page: usize = parts
            .next()
            .map_or(Ok(1), |page| page.parse())
            .map_err(|_| format!("{file}: page number is not a number"))?;
        let path = PathBuf::from(file);
        let name = format!(
            "{}#{}",
            path.file_stem()
                .map(|stem| stem.to_string_lossy().into_owned())
                .unwrap_or_else(|| file.to_owned()),
            page
        );
        cases.push(Case { name, path, page });
    }
    Ok(cases)
}

fn read_tolerances(path: &Path) -> Result<std::collections::BTreeMap<String, Distance>, String> {
    let text =
        std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut tolerances = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let (Some(name), Some(over), Some(mean)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(format!("{}: malformed line `{line}`", path.display()));
        };
        let over = over.parse().map_err(|_| format!("{name}: bad over"))?;
        let mean = mean.parse().map_err(|_| format!("{name}: bad mean"))?;
        tolerances.insert(name.to_owned(), Distance { mean, over });
    }
    Ok(tolerances)
}

fn write_tolerances(path: &Path, calibrated: &[(String, Distance)]) -> Result<(), String> {
    let mut text = String::from(
        "# Inter-oracle spread measured by `pdf2-oracle --calibrate`.\n\
         # case spread_over_pixels spread_mean_channel\n",
    );
    for (name, spread) in calibrated {
        text.push_str(&format!("{name} {:.6} {:.6}\n", spread.over, spread.mean));
    }
    std::fs::write(path, text).map_err(|error| format!("{}: {error}", path.display()))
}

/// The median of our distances to the oracles: the "median oracle" comparison.
fn median_distance(distances: &[Distance]) -> Distance {
    if distances.is_empty() {
        return Distance {
            mean: 0.0,
            over: 0.0,
        };
    }
    let mut over: Vec<f64> = distances.iter().map(|distance| distance.over).collect();
    let mut mean: Vec<f64> = distances.iter().map(|distance| distance.mean).collect();
    over.sort_by(f64::total_cmp);
    mean.sort_by(f64::total_cmp);
    Distance {
        over: over[over.len() / 2],
        mean: mean[mean.len() / 2],
    }
}

fn render_ours(document: &Document, page: usize, scale: f64) -> Result<Ppm, String> {
    let rendered = render_page_number(
        document,
        page,
        &RenderOptions {
            scale,
            ..RenderOptions::default()
        },
    )
    .map_err(|error| error.to_string())?;
    let width = rendered.canvas.width();
    let height = rendered.canvas.height();
    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 3);
    for y in 0..height {
        for x in 0..width {
            let [r, g, b, _] = rendered.canvas.pixel(x, y).expect("pixel inside canvas");
            pixels.extend_from_slice(&[r, g, b]);
        }
    }
    Ok(Ppm {
        width,
        height,
        pixels,
    })
}

struct Oracle {
    name: &'static str,
    program: PathBuf,
}

struct Oracles {
    list: Vec<Oracle>,
}

impl Oracles {
    fn detect() -> Self {
        let mut list = Vec::new();
        for (name, program) in [("poppler", "pdftoppm"), ("mupdf", "mutool"), ("gs", "gs")] {
            if let Some(path) = which(program) {
                list.push(Oracle {
                    name,
                    program: path,
                });
            }
        }
        Self { list }
    }

    fn len(&self) -> usize {
        self.list.len()
    }

    fn names(&self) -> Vec<&'static str> {
        self.list.iter().map(|oracle| oracle.name).collect()
    }
}

impl Oracle {
    fn render(&self, pdf: &Path, page: usize, dpi: f64) -> Result<Ppm, String> {
        let stem = format!(
            "pdf2-oracle-{}-{}-{}",
            std::process::id(),
            self.name,
            pdf.file_stem().unwrap_or_default().to_string_lossy()
        );
        let out = std::env::temp_dir().join(format!("{stem}.ppm"));
        let status = match self.name {
            "poppler" => Command::new(&self.program)
                .args([
                    "-r",
                    &dpi.to_string(),
                    "-f",
                    &page.to_string(),
                    "-l",
                    &page.to_string(),
                    "-singlefile",
                ])
                .arg(pdf)
                .arg(out.with_extension(""))
                .status(),
            "mupdf" => Command::new(&self.program)
                .args(["draw", "-r", &dpi.to_string(), "-o"])
                .arg(&out)
                .arg(pdf)
                .arg(page.to_string())
                .status(),
            _ => Command::new(&self.program)
                .args([
                    "-q",
                    "-dNOPAUSE",
                    "-dBATCH",
                    "-dSAFER",
                    "-sDEVICE=ppmraw",
                    &format!("-r{dpi}"),
                    &format!("-dFirstPage={page}"),
                    &format!("-dLastPage={page}"),
                    &format!("-sOutputFile={}", out.display()),
                ])
                .arg(pdf)
                .status(),
        }
        .map_err(|error| format!("{} failed to start: {error}", self.name))?;
        if !status.success() {
            return Err(format!("{} exited with {status}", self.name));
        }
        let bytes = std::fs::read(&out).map_err(|error| format!("{}: {error}", out.display()))?;
        let _ = std::fs::remove_file(&out);
        Ppm::parse(&bytes).map_err(|error| format!("{}: {error}", self.name))
    }
}

fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(program);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

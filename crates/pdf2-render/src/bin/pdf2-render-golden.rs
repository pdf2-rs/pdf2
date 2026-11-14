//! Render the golden fixture and compare it with the committed PNG.
//!
//! ```text
//! cargo run -p pdf2-render --bin pdf2-render-golden            # verify
//! cargo run -p pdf2-render --bin pdf2-render-golden -- --bless # regenerate
//! ```
//!
//! Exit codes: 0 when the image matches (or was blessed), 1 when it differs,
//! 2 when the fixture cannot be rendered.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pdf2::Document;
use pdf2_render::{encode_png, render_page_number, RenderOptions};

const FIXTURE: &str = "tests/fixtures/paths.pdf";
const GOLDEN: &str = "tests/fixtures/golden/paths.png";

const USAGE: &str = "\
pdf2-render-golden — render the golden fixture and compare it with the PNG

Usage:
  pdf2-render-golden [options]

Options:
  --bless            write the rendered PNG instead of comparing
  --out <path>       where the PNG lives (default tests/fixtures/golden/paths.png)
  --fixture <path>   the PDF to render (default tests/fixtures/paths.pdf)
  --page <n>         page number, counting from 1 (default 1)
  --scale <factor>   pixels per point (default 1.0)
  --help, -h         this help
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err((code, message)) => {
            eprintln!("error: {message}");
            ExitCode::from(code)
        }
    }
}

fn run() -> Result<(), (u8, String)> {
    let mut bless = false;
    let mut out = repo_path(GOLDEN);
    let mut fixture = repo_path(FIXTURE);
    let mut page = 1usize;
    let mut scale = 1.0_f64;

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--help" | "-h" => {
                print!("{USAGE}");
                return Ok(());
            }
            "--bless" => {
                bless = true;
                index += 1;
            }
            "--out" => {
                out = PathBuf::from(value(&arguments, index)?);
                index += 2;
            }
            "--fixture" => {
                fixture = PathBuf::from(value(&arguments, index)?);
                index += 2;
            }
            "--page" => {
                page = value(&arguments, index)?
                    .parse()
                    .map_err(|_| (2, "`--page` needs a number".to_owned()))?;
                index += 2;
            }
            "--scale" => {
                scale = value(&arguments, index)?
                    .parse()
                    .map_err(|_| (2, "`--scale` needs a number".to_owned()))?;
                index += 2;
            }
            other => return Err((2, format!("unknown option `{other}`\n\n{USAGE}"))),
        }
    }
    if page == 0 {
        return Err((2, "`--page` counts from 1".to_owned()));
    }

    let bytes =
        std::fs::read(&fixture).map_err(|error| (2, format!("{}: {error}", fixture.display())))?;
    let document = Document::open(bytes).map_err(|error| (2, error.to_string()))?;
    let rendered = render_page_number(
        &document,
        page - 1,
        &RenderOptions {
            scale,
            ..RenderOptions::default()
        },
    )
    .map_err(|error| (2, error.to_string()))?;
    let png = encode_png(&rendered.canvas);
    println!(
        "rendered {}×{} px ({} bytes), unsupported: {}",
        rendered.canvas.width(),
        rendered.canvas.height(),
        png.len(),
        rendered.report
    );

    if bless {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| (2, format!("{}: {error}", parent.display())))?;
        }
        std::fs::write(&out, &png).map_err(|error| (2, format!("{}: {error}", out.display())))?;
        println!("blessed {}", out.display());
        return Ok(());
    }

    let golden = std::fs::read(&out).map_err(|error| (2, format!("{}: {error}", out.display())))?;
    if golden == png {
        println!("ok: {} matches the renderer", out.display());
        return Ok(());
    }
    let difference = golden
        .iter()
        .zip(&png)
        .position(|(left, right)| left != right)
        .unwrap_or_else(|| golden.len().min(png.len()));
    Err((
        1,
        format!(
            "{} differs from the renderer (golden {} bytes, rendered {} bytes, first difference at byte {difference}); re-run with --bless if the change is intended",
            out.display(),
            golden.len(),
            png.len()
        ),
    ))
}

fn value(arguments: &[String], index: usize) -> Result<&str, (u8, String)> {
    arguments
        .get(index + 1)
        .map(String::as_str)
        .ok_or_else(|| (2, format!("`{}` needs a value", arguments[index])))
}

/// A path inside the repository, independent of the working directory.
fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(relative)
}

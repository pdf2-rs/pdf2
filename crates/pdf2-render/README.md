# pdf2-render

Rasteriser for the [`pdf2`](https://crates.io/crates/pdf2) crate: it turns a page
into RGBA8 pixels using the library's own tokenizer and document model — there is
no second PDF parser here.

```rust
use pdf2::Document;
use pdf2_render::{render_page_number, RenderOptions};

let document = Document::open(std::fs::read("page.pdf")?)?;
let rendered = render_page_number(&document, 0, &RenderOptions::default())?;
std::fs::write("page.png", pdf2_render::encode_png(&rendered.canvas))?;
println!("unsupported operators: {}", rendered.report);
# Ok::<(), pdf2::Error>(())
```

## What it draws

| Area | Support |
| --- | --- |
| Paths | `m`, `l`, `c` (flattened in 24 steps), `v`, `y`, `h`, `re` |
| Fills | non-zero (`f`, `F`, `B`, `b`) and even-odd (`f*`, `B*`, `b*`) |
| Strokes | line width, butt/round/square caps, round joins, dash patterns with a phase |
| Clipping | `W` and `W*` as a per-pixel mask, restored by `q`/`Q` |
| Transform | `cm` composed the way PDF composes it |
| Colour | `g`/`G`, `rg`/`RG`, and the naive `k`/`K` CMYK conversion |
| Text and images | **not yet** — counted per page in `Report::unsupported` |

Rendering is deliberately without anti-aliasing: coverage is decided at pixel
centres, so a golden image is exact and the scanline code stays small. Curves are
flattened with a fixed number of steps, and joins are drawn round whatever the
content stream asks for; both are documented limitations rather than surprises.

## Golden images

`tests/fixtures/paths.pdf` is drawn by `tests/fixtures/make_fixtures.py`; its
rendering is committed as `tests/fixtures/golden/paths.png` and compared
byte-for-byte in the test suite:

```sh
cargo run -p pdf2-render --bin pdf2-render-golden             # verify
cargo run -p pdf2-render --bin pdf2-render-golden -- --bless  # regenerate
```

`--out`, `--fixture`, `--page` and `--scale` make the tool usable on other files.

## Limits

Paths only, plus clips and dashes: text glyphs need a font backend and images
need the image layer, both on the roadmap. Colour is RGB (CMYK converted
naively); there is no colour management.

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
| Text | `BT`/`ET`, `Tf`, `Td`/`TD`/`T*`/`Tm`, `TL`, `Tc`/`Tw`/`Tz`/`Ts`, `Tj`, `TJ`, `'`, `"`; simple-font encodings from `pdf2::Encoding` |
| Fonts | embedded TrueType through `ttf-parser`; otherwise the built-in 5×7 base-14 fallback (lowercase uses uppercase shapes) |
| Images | `/Image` XObjects with `Do`, 8 bits per component, Gray/RGB/CMYK/Indexed/ICCBased, nearest and bilinear sampling |

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

- Anti-aliasing is four sub-scanlines per row with exact horizontal coverage; the
  vertical error is the one left, and `corpus/aa-ceilings.tsv` gates it at 1×1.
  Curves are flattened in fixed steps.
- The built-in font is a fallback: real typography needs an embedded program.
  TrueType, CFF, Type 1 and Type 3 programs are all drawn; a base-14 font without
  an embedded program falls back to a 5×7 alphabet.
- Images: 8 bits per component only. JPEG (`dct`, on by default), CCITT, the
  colour-key `/Mask`, and JPEG 2000 and JBIG2 behind the **`jpx`** and **`jbig2`**
  features (off by default — they cost ~78 and ~64 KiB of gzip on the wasm bundle,
  ADR 0007). A stream `/Mask` (a stencil image) is refused.
- Colour: RGB with a naive CMYK conversion, `/Separation`/`/DeviceN` through their
  tint functions, `/Indexed` and `/ICCBased` by component count. There is no ICC
  colour management; `docs/RENDERING-QUALITY.md` measures what that costs.
- Everything the renderer cannot draw is *reported* — `Report::unsupported` names
  the operator — rather than drawn wrong in silence.

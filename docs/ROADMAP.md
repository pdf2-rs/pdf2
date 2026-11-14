# Roadmap: alpha → beta → rc → 1.0

This crate has no consumers yet, so the plan is to *manufacture* them: every
phase below ships software built on the published version, and each phase has
exit criteria that can be checked rather than argued about. The alpha line is
for finding the API; beta is for the environments (no_std, wasm) and rendering;
rc is for freezing; 1.0 is the promise that the API and the file-format
behaviour will hold.

Everything here is additive to the current scope. The PDF 2.0 read/write core
already exists; what changes is the surface around it and the environments it
runs in.

## Principles

1. **Every claim has a dependent.** A release is not "done" until a released
   tool in this workspace uses it: `pdf2-cli`, `pdf2-render` or `pdf2-wasm`.
   Their CI installs the published `pdf2` and fails if it does not work.
2. **Gates, not dates.** A phase ends when its checklist is green, and its
   artifacts are on crates.io / npm.
3. **`#![forbid(unsafe_code)]` stays.** no_std and wasm do not need unsafe; the
   wasm glue lives in a separate crate where `wasm-bindgen` generates it.
4. **Breaking changes are cheap until beta**, measured and documented during
   beta, and frozen at rc.
5. **Performance and memory are budgets**, recorded as absolute numbers in
   `docs/PERFORMANCE.md` and enforced in CI against the stored baseline.

## Target workspace

```
Cargo.toml                 # [package] pdf2 + [workspace] members = ["crates/pdf2-cli", …]
crates/pdf2-cli            # info, pages, text, check
crates/pdf2-render         # rasterizer: paths, clips, images, text outlines
crates/pdf2-wasm           # wasm-bindgen wrapper + demo (npm package)
crates/pdf2-bench          # benchmark binary + corpus runner (no dependencies)
fuzz                        # cargo-fuzz targets + committed seed corpus (not published)
xtask                      # corpus, size budgets, semver checks, release glue
```

The library stays at the repository root — `src/`, `tests/` and the fixtures do
not move, so `pdf2` remains one crate on crates.io and the published paths keep
working — while the repository root is also the workspace root. Tools live under
`crates/`, sharing the fixtures, CI and the single `rust-toolchain.toml`.

## Deliverables

| Crate | Proves | Published as |
| --- | --- | --- |
| `pdf2` | the file format: parse, model, text, write, decrypt | crates.io (continues the current line) |
| `pdf2-cli` | the library is usable by a program: batch work, real files, exit codes, `--json` | crates.io, `cargo install` |
| `pdf2-render` | the content stream actually paints: paths, clips, transforms, images, glyph outlines | crates.io, usable as a library |
| `pdf2-wasm` | runs in a browser, memory and size bounded | crates.io + npm + GitHub Pages demo |
| `pdf2-bench` | numbers are measured, not claimed | internal, results in `docs/PERFORMANCE.md` |

## Environment matrix

| Target | Feature set | Phase |
| --- | --- | --- |
| `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin` | `std` (default) | now |
| `wasm32-unknown-unknown` | `std` (wasm32) | alpha.3 |
| `wasm32-wasip1` | `std` | alpha.3 |
| `thumbv7em-none-eabihf` (bare metal) | `--no-default-features --features alloc` | alpha.3 |
| `x86_64-unknown-linux-gnu` | `--no-default-features --features alloc` | alpha.3 |

Feature plan for the library:

```toml
[features]
default = ["std"]
std = []            # Document::open_path/save_to_path, std::io errors
alloc = []          # everything else, including the parser
```

Dependency work required for `alloc`:

- **`flate2` → `miniz_oxide`.** flate2 1.1 is `#![no_std]` but still links
  `std` and its `read`/`write` adapters live behind `std::io`. The two call
  sites (`filter.rs` inflate, `writer.rs` deflate) map directly onto
  `miniz_oxide::inflate::decompress_to_vec*` and
  `miniz_oxide::deflate::compress_to_vec_zlib`, which are `core + alloc`.
  This also removes a dependency.
- **Collections move to `alloc`.** `HashMap`/`HashSet` became `BTreeMap`/`BTreeSet`
  from `alloc::collections`, `VecDeque`/`BTreeMap` follow, so the `no_std` build
  needs no new dependency (lookups are `O(log n)` on a few thousand entries).
  `RefCell`, `fmt`, `str::from_utf8` already come from `core`/`alloc`.
- **`std::fs` / `std::path` behind `std`**, as are the `Io` error variant and
  `save_to_path`; `Error` keeps one `#[cfg(feature = "std")]` variant.
- `aes`, `cbc`, `sha2` are already `no_std`-clean.

CI gains `cargo build --no-default-features --features alloc --target
thumbv7em-none-eabihf` plus the wasm targets, and `cargo bloat`/`twiggy` size
reports for the wasm package.

## Phases

### Phase A — proof of use and a baseline (0.3.0-alpha.2)

- Convert the repository to the workspace above; the library stays the root
  package, its tests and fixtures unchanged.
- `pdf2-cli` 0.1.0: `info`, `pages`, `text`, `check`, `--json` everywhere.
  Errors are exit codes, never panics. It reads the committed fixtures in tests
  and a corpus directory when given one.
- `pdf2-bench` + `docs/PERFORMANCE.md`: parse throughput, text extraction,
  memory (peak RSS and allocation counts through a counting global allocator),
  and the command line that produced every number.
- Corpus: the nine committed fixtures, generated stress files from
  `pdf2::Document::new().save()` (100–1000 pages, large streams), and a
  documented place for user-supplied real-world PDFs (git-ignored, never
  required by CI).
- CI: benchmarks run, results compared to the stored baseline, **fail above a
  10% regression**; size report for the library.

**Exit:** `pdf2-cli` on crates.io; `docs/PERFORMANCE.md` has numbers and method;
CLI runs over the corpus with zero panics; the CLI's CI installs the published
`pdf2` and runs it.

### Phase B — environments (0.4.0-alpha.3)

- `std` / `alloc` feature split; the dependency swaps above.
- `no_std + alloc` build green on `thumbv7em-none-eabihf` and
  `x86_64-unknown-linux-gnu`.
- `wasm32-unknown-unknown` and `wasm32-wasip1` builds green; a `wasm` smoke test
  (open, page count, text, decrypt) runs under `wasm-bindgen-test` or Node.
- Documented feature matrix in the README; `Document::open(&[u8])` is the
  supported entry point without `std`.

**Exit:** the matrix above builds in CI; the library compiles to `alloc`-only
and to wasm; the size report is recorded.

### Phase C — rendering and the wasm package (0.5.0-beta.1)

Scope for 1.0 (agreed): **paths, clipping, transforms, images and text
outlines**. Type 3 fonts are limited to their glyph procedures; ICC colour
management and JPEG/JPX decoding stay out of scope unless a later minor adds
them behind features.

- `pdf2-render` 0.1.0 (no_std-friendly core, `std` for I/O conveniences):
  - graphics state: CTM stack, clipping paths, fill/stroke, line joins/caps,
    dash patterns, even-odd and non-zero winding;
  - images: `/Image` XObjects, Flate/ASCII/`LZW`/`RunLength` payloads, all
    `/ColorSpace` variants that the core parser exposes, nearest/bilinear
    sampling;
  - text: glyph outlines through a pure-Rust font crate (`ttf-parser` for
    TrueType/OpenType, `Type1`/CFF via the same crate where available), simple
    and composite fonts; the crate takes a font provider so a caller can supply
    system fonts;
  - output: RGBA8 and grayscale, configurable resolution and page box.
  - Golden-image tests: small pages rendered to committed PNGs, compared
    pixel-for-pixel, plus a diff tool for intentional changes.
- `pdf2-wasm` 0.1.0: `Document` (open with optional password), `pageCount`,
  `pageSize`, `text`, `metadata`, `renderPage(index, scale) -> Uint8ClampedArray`
  and `renderPageToCanvas`. Published to crates.io and npm
  (`@pdf2/pdf2-wasm`), with a static demo on GitHub Pages that opens a bundled
  fixture and a user-selected file.
- Size and memory budgets for the wasm package: set from the first measurement,
  then ratcheted; CI fails if the gzipped bundle grows past the budget or if a
  render of the reference page exceeds its recorded peak.

**Exit:** `pdf2-render` and `pdf2-wasm` released; the demo runs in a browser;
golden images pass; both dependents install the published `pdf2`.

### Phase D — hardening (0.6.0-beta.2 … beta.N)

- Fuzzing: `pdf2-fuzz` targets for the parser, xref reader, filter layer,
  content-stream interpreter and the decrypt path; nightly job, 24 h clean run,
  corpus committed. Any crash becomes a regression test in
  `tests/robustness.rs`.
- `cargo-semver-checks` against the previous release; `cargo deny` for licences
  and advisories; MSRV job kept green.
- API review: every public item documented, examples for each entry point,
  `#![warn(missing_docs)]` enforced.
- Security review of the security handler (password hashing, key derivation,
  `/Perms`), and of the allocation behaviour on hostile input (no unbounded
  vector growth from declared counts).
- Performance gates promoted from "report" to "fail" for the wasm package too.

**Exit:** fuzz clean for 24 h, semver-check clean, docs coverage at 100% of the
public surface, no open P1 bugs.

### Phase E — freeze (0.7.0-rc.1)

- API freeze: only bug fixes and internal changes after the first rc. Anything
  else waits for 1.1.
- Migration guide from 0.2.0-alpha.1, including the feature flags.
- Release checklist in `docs/RELEASING.md` extended for the workspace publishing
  order (`pdf2` → `pdf2-render` → `pdf2-cli` → `pdf2-wasm`).
- Docs: README, SPEC-MAP, PERFORMANCE, ROADMAP and CHANGELOG consistent; every
  crate has its own README and `docs.rs` landing page.

**Exit:** 14 days with no API change; one rc used by all three dependents.

### Phase F — 1.0.0

- `pdf2` 1.0.0, `pdf2-render` 1.0.0, `pdf2-cli` 1.0.0, `pdf2-wasm` 1.0.0 —
  each semver-independent from then on.
- A written compatibility policy: what is covered by semver (public API,
  supported file-format behaviour) and what is not (rendering fidelity beyond
  the documented scope, performance numbers).
- Announcement with the demo, benchmarks and the dependent tools.

## Performance and memory method

- One command reproduces every published number, on a named machine with the
  commit hash recorded: `cargo xtask bench --corpus <dir>`.
- Metrics: parse MB/s, text extraction pages/s, object fetch latency, render
  pages/s at 1x/144 dpi, peak RSS per page, total allocations per page, wasm
  gzipped size, `no_std` binary contribution.
- `docs/PERFORMANCE.md` holds: the corpus description, the command, absolute
  numbers (median of N runs), and the budget each number is held to.
- CI stores the baseline in the repository; a run over budget fails with the
  diff. Budgets ratchet down deliberately, never up silently.

## Quality gates that run at every release

- `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`,
  `cargo test` (unit, integration, doc-tests), `cargo doc` with warnings denied.
- MSRV 1.87 and beta (and, for the library, the `alloc` and wasm targets).
- `tests/robustness.rs` sweeps over every fixture, unchanged.
- Workspace tests install the *published* versions of the sibling crates, so a
  broken release is a red build, not a user report.

## Out of scope for 1.0

Rendering fidelity matching Acrobat, PDF/A conformance, forms, JavaScript, XFA,
digital signatures, encryption *writing*, JPEG/JPX/CCITT/JBIG2 decoding and ICC
colour management. Each would be a later minor version with its own feature
flag, and none of them blocks 1.0.

## Risks

| Risk | Mitigation |
| --- | --- |
| `no_std` refactor ripples through the public API (`std::io` errors, `Path`) | do it in alpha.3, before the API freeze; keep `Io` gated behind `std` |
| Font rasterization is a large surface (Type1, CFF, TrueType, composite CMaps) | buy it: use `ttf-parser`-class crate rather than writing one; limit 1.0 to what it supports and document gaps |
| Rendering correctness is hard to prove without a reference renderer | golden images committed with the fixtures; manual review of a small reference set; no claim of Acrobat parity |
| Benchmarks on one machine are noisy | median of N runs, same runner, recorded hardware; fail only on ≥10% regressions |
| wasm memory growth on large pages | render in tiles, document a page-size ceiling, enforce a peak-memory budget in CI |
| Release train stalls waiting for rendering | alpha.2 and alpha.3 ship without any rendering work; rendering is additive in beta |

## Next actions

1. Convert to the workspace with the library as the root package, so no source
   moves.
2. Land `pdf2-cli` with `info`, then `pages`/`text`/`check`, then the benchmark
   harness and the first `docs/PERFORMANCE.md`.
3. Feature-gate `std`, swap `flate2` for `miniz_oxide`, and add the `alloc` and
   wasm targets to CI.

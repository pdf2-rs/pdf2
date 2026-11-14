# Migrating from 0.2.0-alpha.1

The alpha.2 line rebuilt the crate around two things the first alpha did not
have: a `no_std` build and a shared encoding table for text. Nothing public was
removed, so most users can bump the version and stop reading.

## Cargo.toml

`pdf2` now has features. The default is unchanged: `std`.

```toml
[dependencies]
pdf2 = "0.2.0-alpha.2"
```

For a `no_std` target (`core` + `alloc`, no file system):

```toml
[dependencies]
pdf2 = { version = "0.2.0-alpha.2", default-features = false, features = ["alloc"] }
```

`Document::open(&[u8])` and the in-memory writer are the entry points without
`std`. `Document::open_path`, `open_path_with_password` and
`DocumentBuilder::save_to_path` exist only with `std`, and so does the
`From<std::io::Error>` conversion for `pdf2::Error`.

## API additions

- `pdf2::Encoding`, with `Encoding::standard()`, `Encoding::win_ansi()`,
  `Encoding::from_name`, `Encoding::from_object`, `Encoding::apply_differences`
  and `Encoding::character`. It is the mapping text extraction already used;
  it is public now because the renderer needs it too.
- `pdf2::Object::as_real`, used by the content interpreter.

Nothing was renamed or removed. `Error` is still `#[non_exhaustive]`, so matches
outside the crate already need a catch-all arm.

## Behaviour

Text extraction, page-tree inheritance, metadata and decryption are unchanged;
the whole fixture corpus and the renderer's golden images pin them.

Dependencies changed underneath without touching the API: `flate2` was replaced
by `miniz_oxide`, and the core uses `alloc`-only collections. The `pdf2` public
types (`Dictionary`, `Object`, …) are the same.

## Related crates

The tools that live in this repository moved forward with the library:

| Crate | Version |
| --- | --- |
| `pdf2-cli` | `0.1.0-alpha.1` |
| `pdf2-render` | `0.1.0-alpha.1` |
| `pdf2-wasm` | `0.1.0-alpha.1` |

See `docs/PERFORMANCE.md` for the benchmarks and `docs/ROADMAP.md` for where the
0.3 → 1.0 line is going.

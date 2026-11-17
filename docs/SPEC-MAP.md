# ISO 32000-2 clause map

Which part of the crate implements which clause of ISO 32000-2 (PDF 2.0). The
spec itself is not committed here — it is available free of charge from the PDF
Association at <https://pdfa.org> and stays out of version control. This file
records only clause numbers and names, so the code can be checked against the
standard without vendoring it. Where a sub-clause number matters, confirm it
against that free copy: the numbering below follows the standard's table of
contents as it stood at ISO 32000-1 for the chapters 2.0 kept unchanged.

## Verified for the deferred plans

The clause numbers the plans in [`docs/plans/`](plans/README.md) rely on. Each was
checked against the published 2020 edition through the PDF Association's errata
(`pdf-issues.pdfa.org/32000-2-2020/…`), which quotes the clause headings, rather than
recalled — and the file naming is what a plan must cite.

| Clause | Name | Needed by |
| --- | --- | --- |
| `7.4.7` | JBIG2Decode filter | `jpx-jbig2.md` |
| `7.4.9` | JPXDecode filter | `jpx-jbig2.md` |
| `7.5.6` | Incremental updates | `editing.md` |
| `8.5.3.3` | Filling (non-zero winding) | `tr-clipping.md` |
| `8.6.5.5` | ICCBased colour spaces | `icc.md` |
| `8.6.5.8` | Rendering intents | `icc.md` |
| `8.7.3` | Tiling patterns | `tiling-patterns.md` |
| `8.7.4` | Shading patterns | `tiling-patterns.md`, `mesh-shadings.md` |
| `8.7.4.5.5`–`.8` | Shading types 4–7 (mesh) | `mesh-shadings.md` |
| `8.9.6.4` | Colour key masking | `colour-key-mask.md` |
| `9.3.6` | Text rendering mode | `tr-clipping.md` |
| `10.4` | Conversions among device colour spaces | `icc.md` |
| `12.5.5` | Appearance streams | `forms.md` |
| `12.7.2`, `12.7.3` | Interactive form dictionary, field dictionaries | `forms.md` |

Status values: **done** = implemented and covered by tests, **partial** = the
happy path works and edge cases are planned, **planned** = not started, with the
round from the objective file that will deliver it.

## Lexical and object syntax (§7.2, §7.3)

| Clause | Name | Module | Status |
| --- | --- | --- | --- |
| 7.2.2 | Character set, whitespace, delimiters | `src/lexer.rs` (`is_whitespace`, `is_delimiter`, `is_regular`) | done |
| 7.2.3 | Comments | `src/lexer.rs` (`Lexer::skip_trivia`) | done |
| 7.3.2 | Boolean and null objects | `src/object.rs`, `src/parser.rs` | done |
| 7.3.3 | Numeric objects (integers, reals) | `src/lexer.rs` (`parse_number`), `src/object.rs` (`real_to_string`) | done |
| 7.3.4.2 | Literal strings, escapes, EOL normalisation | `src/lexer.rs` (`literal_string`, `string_escape`) | done |
| 7.3.4.3 | Hexadecimal strings | `src/lexer.rs` (`hex_string`) | done |
| 7.3.5 | Name objects, `#xx` escapes | `src/lexer.rs` (`name`), `src/object.rs` (`write_name`) | done |
| 7.3.6 | Array objects | `src/parser.rs` (`parse_array`) | done |
| 7.3.7 | Dictionary objects | `src/parser.rs` (`parse_dictionary`), `src/object.rs` (`Dictionary`) | done |
| 7.3.8 | Stream objects, `/Length` | `src/parser.rs` (`parse_stream`), `src/object.rs` (`Stream`), `src/writer.rs` (`write_stream_object`) | read + write |
| 7.3.9 | Direct objects (the `n g R` reference form) | `src/parser.rs` (`parse_integer_or_reference`) | done |
| 7.3.10 | Indirect objects (`n g obj … endobj`) | `src/parser.rs` (`parse_indirect_object`) | done |

## File structure (§7.5)

| Clause | Name | Module | Status |
| --- | --- | --- | --- |
| 7.5.2 | File header (`%PDF-2.0`, binary comment) | `src/document.rs` (`header_version`), `src/writer.rs` | read + write |
| 7.5.3 | Cross-reference table, subsections, free entries | `src/document.rs` (`parse_xref_section`) | done |
| 7.5.4 | Cross-reference streams (`/Type /XRef`, `/W`, `/Index`) | `src/document.rs` (`parse_xref_stream`, `xref_widths`, `xref_index`), `src/writer.rs` (`xref_rows`) | read + write |
| 7.5.5 | File trailer, `startxref`, `%%EOF` | `src/document.rs` (`startxref_offset`, `read_xref_chain`), `src/writer.rs` (`save`) | read + write |
| 7.5.6 | Incremental updates, `/Prev` chains | `src/document.rs` (`read_xref_chain`), `tests/fixtures/classic-xref.pdf` | done |
| 7.5.7 | Object streams (`/ObjStm`, `/N`, `/First`, type-2 entries) | `src/document.rs` (`load_object_stream`, `compressed_object`), `src/writer.rs` (`pack_object_stream`) | read + write |
| 7.5.8 | Hybrid-reference files (`/XRefStm`) | `src/document.rs` (`read_xref_chain`) | done |
| 7.8.2 | Content streams: operand stack, operators, inline-image skipping | `src/content.rs` (`TextInterpreter`) | partial — text operators interpreted, graphics state not modelled |

## Filters (§7.4)

| Clause | Name | Module | Status |
| --- | --- | --- | --- |
| 7.4.2 | FlateDecode (zlib, with a raw-DEFLATE fallback) | `src/filter.rs` (`flate_decode`) | done |
| 7.4.3 | LZWDecode, `/EarlyChange` | `src/filter.rs` (`lzw_decode`) | done |
| 7.4.4 | ASCIIHexDecode, ASCII85Decode | `src/filter.rs` (`ascii_hex_decode`, `ascii85_decode`) | done |
| 7.4.4.4 | Predictor functions (PNG 10–15, TIFF 2), `/DecodeParms` | `src/filter.rs` (`png_predictor`, `tiff_predictor`, `Predictor`) | done |
| 7.4.5 | RunLengthDecode | `src/filter.rs` (`run_length_decode`) | done |
| 7.3.8 | `/Filter` arrays, `/DecodeParms` arrays, indirect entries | `src/filter.rs` (`FilterSpec`), `src/document.rs` (`decode_stream`) | done |

## Document structure, graphics and text (§7.7, §8, §9, §14)

| Clause | Name | Module | Status |
| --- | --- | --- | --- |
| 7.7.2 | Document catalog | `src/document.rs` (`catalog`, `catalog_dict`) | done |
| 7.7.3 | Page tree, inherited `/MediaBox`/`/CropBox`/`/Rotate`/`/Resources`, `/Count` not trusted | `src/page.rs` (`Document::pages`, `collect_pages`) | done |
| 8.2 | Graphics objects and operators | `src/content.rs` | partial — operands consumed and ignored; nothing is painted |
| 9.3 | Text state: `Tf`, `Td`, `TD`, `T*`, `TL`, `Tm`, `Tc`, `Tw`, `Tz`, `Ts` | `src/content.rs` (`TextInterpreter::execute`) | done for extraction (spacing values recorded but unused) |
| 9.4 | Text objects, `Tj`, `TJ`, `'`, `"`, text extraction | `src/content.rs` (`show`, `adjust`) | done |
| 9.6 | Simple fonts, `/Encoding` (`StandardEncoding`, `WinAnsiEncoding`), `/Differences` | `src/encoding.rs` (`Encoding`), shared by extraction and rendering | partial — ASCII and the cp1252 block; MacRoman upper half and unknown glyph names are skipped |
| 9.7 | Composite (Type0) fonts, `/ToUnicode` CMaps | — | deliberately unsupported: reported as an error |
| 14.3.2 | Metadata streams (XMP) | `src/page.rs` (`Document::metadata`, `has_metadata`), `src/writer.rs` (`XMP_PACKET`) | read + write (no `/Info` on either side) |

## Painting, outside the library

`pdf2-render` walks the content stream with `pdf2::Parser` and paints it. The
clauses it covers (see [RENDERING.md](RENDERING.md) for the fixture that proves
each one, and for what is still unimplemented):

| Clause | Name | Module | Status |
| --- | --- | --- | --- |
| 8.2 | Graphics objects, operators, path construction | `crates/pdf2-render/src/walk.rs`, `path.rs` | done for the operators in the crate README |
| 8.5 | Fill and stroke: non-zero and even-odd rules, caps, joins, dashing | `crates/pdf2-render/src/raster.rs`, `path.rs` | done, round joins only |
| 8.9 | Images: `/Image` XObjects, sampling | `crates/pdf2-render/src/image.rs` | 8-bit Gray/RGB/CMYK/Indexed/ICCBased; no `/SMask` |
| 9.3, 9.4 | Text state and text objects, glyph outlines | `crates/pdf2-render/src/font.rs`, `walk.rs` | simple fonts; embedded TrueType via `ttf-parser`, built-in fallback |
| 14.11.2 | Clipping paths | `crates/pdf2-render/src/raster.rs` (`clip_mask`) | rectangular and path clips as a pixel mask |

## Encryption (§7.6)

| Clause | Name | Module | Status |
| --- | --- | --- | --- |
| 7.6.2 | Encryption dictionary (`/V 5`, `/R 6`, `/U`, `/O`, `/UE`, `/OE`, `/Perms`) | `src/crypto.rs` (`Encryption::new`) | done |
| 7.6.4 | Standard security handler: file key from `/U` or `/OE`, `/EncryptMetadata`, strings and streams | `src/crypto.rs`, `src/document.rs` (`open_with_password`, `decode_stream`) | done |
| 7.6.4.3.3 | Algorithm 2.B — revision 6 password hashing (SHA-256/384/512 + AES-128-CBC) | `src/crypto.rs` (`hash_r6`) | done |
| 7.6.1 | Exceptions: cross-reference streams never encrypted, `/Metadata` in the clear when `/EncryptMetadata false` | `src/crypto.rs` (`decrypt_stream`) | done |

## Dependencies

| Crate | Why | Clause |
| --- | --- | --- |
| `flate2` | zlib/DEFLATE for FlateDecode | 7.4.2 |
| `aes`, `cbc` | AES-128 (password hashing) and AES-256 (strings, streams, `/Perms`) | 7.6.4.3.3 |
| `sha2` | SHA-256/384/512 for Algorithm 2.B | 7.6.4.3.3 |

Everything else (LZW, ASCIIHex, ASCII85, RunLength, predictors, cross-reference
parsing, encryption to come) is implemented in this crate so the file format is
under test end to end.

Deliberately absent: RC4 and AES-128 (§7.6.4 revisions 2–4), the `Info`
dictionary (§14.3.3) and everything the objective file lists as out of scope —
rendering, PDF/A, XFA, JavaScript and form filling.

## Tests and fixtures

| Path | Purpose |
| --- | --- |
| `src/**/tests` | Unit tests next to the code they cover |
| `tests/objects.rs` | `corpus()` — inline text ↔ `Object` pairs used as the byte-exact round-trip fixture, plus malformed and truncated input cases |
| `tests/xref_streams.rs` | Cross-reference streams, object streams, hybrid files, and the classic-twin graph comparison |
| `tests/pages.rs` | Page tree inheritance (`/MediaBox`, `/CropBox`, `/Rotate`, `/Resources`), page count, XMP metadata |
| `tests/writer.rs` | `Document::new()` … `save()` → `Document::open()` round-trips: pages, object graph, offsets, metadata, custom objects |
| `examples/write.rs` | Runable end-to-end writer example (`cargo run --example write`) |
| `tests/encryption.rs` | AES-256 `/V 5 /R 6`: user and owner passwords, wrong-password errors, encrypted/plain text equality, `/Perms` tamper check, deprecated revisions refused |
| `crates/pdf2-render/tests/golden.rs` | Golden images for `paths.pdf` and `images.pdf`, compared byte-for-byte, plus pixel-level assertions |
| `tests/robustness.rs` | every fixture truncated at every length, sampled byte mutations and deletions, exercising the whole read API on anything that still opens |
| `examples/make_encrypted_fixture.rs` | Generates `encrypted-r6.pdf` and `plain-twin.pdf` (no external tool can produce an R6 file here) |
| `tests/fixtures/*.pdf` | Hand-built PDF files, committed, each paired with the assertions it must satisfy; the compressed ones are rebuilt by `tests/fixtures/make_fixtures.py` |

Fixtures are small, hand-written and committed so that `cargo test` proves the
reader against bytes a real writer produces, without any external validator
(none is installed here; see the objective file).

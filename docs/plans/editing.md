# Editing an opened document

Deferred item: **editing** (`docs/DEFERRED.md` §1, rank 11). `Document` is read-only by
construction and `src/writer.rs` only builds new files, so there is no way to change an
existing document at all. The mutation model is decided in
[ADR 0006](../adr/0006-editing-mutation-model.md); this plan is what building it looks like.

## Clause

**`7.5.6` Incremental Updates** (append, an xref section with only the changed/replaced/
deleted objects, a trailer carrying the previous one's entries plus `/Prev`, each trailer
ended by its own `%%EOF`), **`7.5.5` File Trailer**, **`7.5.8` Cross-Reference Streams**
(the alternative form, deferred), and **`14.4` File Identifiers** — the first `/ID` string
is permanent and must survive an update, which an encrypted file's key depends on.

All four are in `docs/SPEC-MAP.md` (`7.5.6` at line 40 as already *read*).

## Verdict

**No dependency, no licence, no wasm cost.** The writer is bytes and arithmetic: the file
is already in memory, the xref parser already exists, and the encryption handlers already
encrypt and decrypt object by object (`src/crypto.rs`). The wasm budget is untouched
because the browser build has no save API and this plan does not add one.

**The cost is correctness, and it is concentrated in three places**: the trailer's
composition (every previous entry *except* `/Prev`, plus the new one), `/ID` (first string
frozen, second replaced), and encryption of the appended objects (revisions 2–4 derive the
key from the object number, so a lazily-written object is a subtly wrong file).

**Prior art that de-risks the shape, not the code**: `lopdf` 0.47.0's
`IncrementalDocument` holds the original bytes, the previously-parsed document and "a new
document appended to the previously loaded file" (`lopdf-0.47.0/src/incremental_document.rs:4-14`)
— the same overlay model ADR 0006 chooses, MIT-licensed, so the approach is proven by a
crate with 30.7M downloads.

## Design

1. **`MutableDocument`** (new module `src/edit.rs`, public from the crate root): owns the
   base `Document` plus `changes: BTreeMap<ObjectId, Object>`, `deleted: BTreeSet<u32>`,
   `next_object: u32`. `get_object` consults the overlay first; a deleted id is absent;
   `set_object(id, object)` and `add_object(object) -> ObjectId` are the write API.
2. **Saving** (`save_incremental(&self) -> Result<Vec<u8>>`): start from a copy of the base
   bytes; append each changed/new object at the current end (encrypting strings and streams
   when the base is encrypted); remember each offset; write **one** classic `xref` section
   whose subsections cover exactly the written numbers, with `f` rows for deletions; write
   the trailer as the base trailer minus `/Prev`, `/Size` raised, `/Prev` = the base's last
   `startxref`, `/ID` with its second string replaced; then `startxref`, the offset, and
   `%%EOF`.
3. **Page-tree editing helper** (`add_page`): rewriting `/Kids` and `/Count` on the `/Pages`
   node is the first real edit and the one every caller wants; it is an overlay write of the
   node, so the plan keeps it thin.
4. **The ceilings** (`object_limit`, `page_limit`) are checked on the *result* — the base
   document's object count plus the overlay — and a save that would exceed one returns
   `Error::TooMany` rather than writing a file this crate then refuses to open.
5. **Refusals stay honest**: an unsupported security handler to encrypt for, a base file
   whose `/ID` is missing while encryption needs it, and a save asked to write nothing are
   each reported, not improvised.

## Acceptance

- **Round trip, measured on the file**: open `tests/fixtures/text.pdf`, add a page whose
  content draws a known string, save incrementally, reopen the *saved bytes* — the page
  count is one higher, the new page's text extracts, and the first page still extracts what
  it did. The test asserts all three, because a save that silently drops a page passes a
  page-count-only check.
- **The original bytes survive**: the saved file **starts with** the input bytes, byte for
  byte — the property that makes signatures valid and data loss impossible. This is one
  `starts_with` assertion and it is the whole point.
- **Another reader agrees**: the saved file is rendered by `pdftoppm` and `mutool` (already
  installed by the oracle job) and both exit 0 with non-blank output. `qpdf --check` would
  be the better structural check and is named as a prerequisite for that job, not assumed.
- **An encrypted file, edited and reopened**: `tests/fixtures/encrypted-r4-aes.pdf` with
  password `user`, add a page, save, reopen with the same password — the page is there and
  the text extracts, which proves the appended objects were encrypted with the *right* key.
- **The ceiling fires on write**: an edit that pushes the object count past
  `object_limit` returns `Error::TooMany` and produces no bytes.
- **No oracle case**: the fixture is generated in the test and compared by re-reading it,
  which is a stronger check than a pixel comparison for a structural feature.

## Rounds

1. **The overlay** — `src/edit.rs`, `MutableDocument`, reading through it, and the
   `add_page` helper, with the round-trip test above but saving nothing yet (assert the
   in-memory result).
2. **The save** — append, xref section, trailer with `/Prev`, `/ID` handling, `%%EOF`; the
   round trip, the `starts_with` property, and the two external readers.
3. **Encryption and the ceilings** — the encrypted round trip, the `Error::TooMany` path,
   and the refusals.
4. **Documentation and the ledger** — `docs/RENDERING.md` (no change), `docs/FEATURES.md`'s
   rank-11 rows, and `README.md`'s status line for the `pdf2-cli` `edit` verb if one is
   added (the CLI work is separate and out of scope here).

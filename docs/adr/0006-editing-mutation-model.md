# ADR 0006 — A mutation model for editing an opened document

- **Status**: accepted, 2026-11-17
- **Decides**: the shape of editing (rank 11) that the forms plan
  ([`docs/plans/forms.md`](../plans/forms.md)) rests on
- **Related**: `docs/SPEC-MAP.md:40` (this crate already *reads* 7.5.6), `docs/DEFERRED.md` §1

## Context

`Document` is read-only by construction, and deliberately: it owns the file's bytes
(`data: Vec<u8>`), the parsed cross-reference map, the trailer, a `RefCell` cache, and the
open security handler — every field is either borrowed or interior-mutable but never
mutable-in-place. `src/writer.rs` builds *new* files (`DocumentBuilder::{add_object,
add_page, save}`), which is the opposite operation: it never sees an existing document.

The demand for the other direction is the largest single number in the register (`lopdf`
30.7M downloads), and forms filling — the next item — cannot exist without it: setting a
field is writing `/V` into an existing object and (usually) regenerating an `/AP`.

The standard's own framing decides the mechanism. **`7.5.6` Incremental Updates**: "changes
shall be appended to the end of the file, leaving its original contents intact", the new
cross-reference section "shall contain entries only for objects that have been changed,
replaced, or deleted", deleted objects are "left unchanged in the file, but shall be marked
as deleted by means of their cross-reference entries", and the appended trailer carries
every entry of the previous one **except `/Prev`**, plus a `/Prev` pointing at the previous
cross-reference section — with each trailer terminated by its own `%%EOF`.

Two clauses constrain the details:

- **`14.4` File Identifiers**: `/ID` is two strings; the first "shall not change when the
  file is incrementally updated", the second changes with each revision. On an encrypted
  file this is not cosmetic — the key derivation mixes `/ID`'s first element, so a writer
  that regenerates it cannot read back what it wrote.
- **`7.5.6`'s NOTE 3**: duplicates of an object id are legal and "the most recent copy of
  each object shall be the one accessed". So deletion is a *new* xref row, never a byte
  edit.

## Decision

1. **Incremental saves only, in the first model.** We append; we never rewrite the original
   bytes and never rebuild the file. A failed save therefore cannot damage the input, and a
   signed document keeps its signature valid (which is what makes editing acceptable at all
   in the standard's framing).
2. **A `MutableDocument` overlays a `Document`**: `changes: BTreeMap<ObjectId, Object>`,
   `deleted: BTreeSet<u32>`, `next_object: u32` (starting at the base `/Size`). Reading
   goes through the overlay first, the base document second; a deleted id reads as absent.
   The base `Document` is never modified, so a caller can keep using it.
3. **Saving appends**: each changed or new object is written at the end of the base bytes
   (encrypted when the base is — see 5), then **one** cross-reference section whose entries
   cover exactly those object numbers (`f` rows for the deleted ones), then a trailer which
   is the base trailer **minus `/Prev`**, with `/Size` raised, `/Prev` set to the base's last
   `startxref`, and `/ID`'s *second* string replaced. Then `startxref`, the offset, and its
   own `%%EOF`.
4. **Classic table by default.** The new section is a classic `xref` table — legal
   regardless of how the original was written, and the simplest thing that a reader will
   accept; an xref stream (`7.5.8`) is an optimisation to add later, not a requirement.
5. **Encryption is carried, not bypassed.** New strings and streams are encrypted with the
   base document's handler (revisions 2–4 derive the key from the *object number* as well as
   `/ID`, so this is per-object), and `/ID`'s first element is copied verbatim. A file we
   cannot encrypt for (an unsupported handler) is refused, not written wrongly. Implemented
   for `/R 4` (RC4 `/V2` and AES-128 `/AESV2`) and `/R 6` (AES-256 `/AESV3`), verified by a
   round trip on all three fixtures: `encrypted-r4-aes.pdf` 1 449 B → 542 B appended,
   `encrypted-r4-rc4.pdf` 1 354 B → 524 B, `encrypted-r6.pdf` 1 694 B → 542 B.
   **The AES IV is derived** — SHA-256 over the key, the object number and the payload,
   truncated to 16 bytes — rather than drawn from a random source, which this library does not
   have (`no_std`, and the choice is not a reader's to make). The consequence is deliberate:
   the same edit saved twice is byte-identical, the same property `/ID`'s second string has,
   and two appends of the *same* payload to the *same* object number would share an IV — which
   an incremental update never does, because object numbers are never reused.
6. **The ceilings apply to writes.** The overlay counts against `object_limit` and any
   page-tree change against `page_limit`, with the same `Error::TooMany` the reader uses. A
   save that would produce a document this crate then refuses to open is a bug, not a file.
7. **No whole-file rewrite mode.** It would lose signatures, need a page-tree rebuild, and
   duplicate the writer we already have for new documents. If a caller wants a compacted
   file, the answer is "build a new one from what you read", which `DocumentBuilder` does.

## Consequences

- The forms plan can be written against this: setting `/V` is an overlay write, and
  flattening is a *render* (which already exists) plus an overlay change.
- A document that was already updated incrementally is handled by following `/Prev` when
  reading the *last* `startxref`, which `read_xref_chain` already does.
- Object numbers are never reused, so a saved file grows monotonically; a document edited a
  thousand times is a thousand appends. Compaction is the caller's decision, not ours.
- The reader's `Error::TooMany` becomes a write-side error too, which is a new place for it
  to fire and needs its own test.

## Alternatives rejected

- **Rewriting the file** (parse, mutate, serialise): loses every signature, risks data loss
  on a partial write, and would have to rebuild the page tree and the object streams for
  no gain over `DocumentBuilder`.
- **A separate crate that shells out to `qpdf`**: violates the portability rule (no C
  dependency, wasm has no processes) and makes the tool, not the library, the product.
- **Editing in place** (the base `Document` gains setters): every read path would have to
  cope with a mutated document plus its original bytes, and the read-only guarantee — the
  thing that makes the reader safe to hand an untrusted file — would be gone.

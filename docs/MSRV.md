# The MSRV: 1.100, and what 1.88–1.100 brought

The floor is **1.100** (`rust-version` in every manifest, `.clippy.toml`'s `msrv`,
the `Test (1.100)` CI job). It was 1.87 until 2026-11-17, and the raise is recorded
in `CHANGELOG.md` §1.4.0 with its reason: the three decoders
[ADR 0004](adr/0004-image-decoders-and-the-msrv.md) deferred need 1.92.

This file is the catalogue that raise owes: one line per release in the range, what
it added that could matter to this crate, and what we did about it — so the next
person does not re-read thirteen release notes to find out we already said no.
Every line is from `rust-lang/rust`'s `RELEASES.md` (fetched 2026-11-17).

| Release | What it brought that could matter | Verdict |
| --- | --- | --- |
| 1.88 | `slice::as_chunks`/`as_chunks_mut`; `Cell::update`; `HashMap::extract_if`; `hint::select_unpredictable`; let-chains **in edition 2024 only**; `cfg` boolean literals; naked functions | **used** — `as_chunks` at ten sites (clippy's `chunks_exact_to_as_chunks` now denies the old form) and let-chains at the thirteen collapsible nested `if let`s; the rest are not applicable (no `HashMap` in the library, no naked functions) |
| 1.89 | `NonZero<char>`; `File::lock` family; `NonNull` provenance methods; `OsString::leak`/`PathBuf::leak`; explicitly inferred const arguments; `mismatched_lifetime_syntaxes` lint | **not applicable** — nothing locks a file, leaks a path or names a const argument. The lint is on by default and the gate would have said so |
| 1.90 | `uN::checked_sub_signed` and friends; `IntErrorKind: Copy + Hash`; `CStr` comparisons | **not applicable** — the parser's arithmetic is on `usize`/`u32` with explicit checks, and there is no `CStr` |
| 1.91 | `Path::file_prefix`; `strict_*` integer arithmetic; `AtomicPtr` fetch-op set; pattern-binding drop order | **refused** — `strict_add`/`strict_sub` would be the honest form for the *untrusted-input* arithmetic (offsets, lengths), but they panic rather than error, and every such site here must return an `Error` instead; noted as a possible follow-up, not taken |
| 1.92 | `NonZero::div_ceil`; `Location::file_as_c_str`; `RwLockWriteGuard::downgrade`; `Box`/`Rc`/`Arc::new_zeroed`; `btree_map::Entry::insert_entry`; never-type fallback lints deny-by-default | **not applicable** here — but it is the release the decoders need, which is *why* the floor is 1.100 rather than 1.92's user-visible API |
| 1.93 | `MaybeUninit` slice methods; `String`/`Vec::into_raw_parts`; `<[T]>::as_array`/`as_mut_array`; unchecked shifts; `cfg` keyword error | **not applicable** — no raw-part plumbing in a `#![forbid(unsafe_code)]` library. `as_array` would be nice where a `&[u8]` of known length is pattern-matched, but no such site paid for the churn |
| 1.94 | `slice::array_windows`; `slice::element_offset`; `LazyCell::get`/`get_mut`; `TryFrom<char> for usize`; `Peekable::next_if_map`; Unicode 17 | **used** — `array_windows::<2>` in the path stroker and the CFF ROS test, `array_windows::<4>` in the bench's fixture check; a fixed window is now a `&[T; N]`, so the two indices inside the loop are typed rather than conventionally in range |
| 1.95 | `if let` guards on match arms; `MaybeUninit<[T; N]>` conversions; `Cell<[T; N]>: AsRef`; `AtomicPtr::update`; `bool: TryFrom<integer>` | **refused, with the reason** — every candidate in the operator dispatch is "do nothing when the operand is absent", and a guard forces a second arm repeating the operator name to say *nothing*, which is longer than the nested `if let` it replaces. The one place a guard reads better (a *mandatory* operand) would turn silence into a report, which is a coverage change, not a sweep |
| 1.96 | `assert_matches!` / `debug_assert_matches!`; `From<T> for LazyCell`/`LazyLock`; `core::range` types; always coerce never types in tuples | **used** — twelve `assert!(matches!(…))` assertions in five test files became `assert_matches!`, which prints the value and the pattern on failure instead of a bare `assertion failed`. It is **not** in the prelude, so each file imports `use std::assert_matches;` |
| 1.97 | `{integer}::bit_width`/`lowest_one`/`highest_one`/`isolate_*`; `Cargo build.warnings` config; `resolver.lockfile-path`; `-m` for `--manifest-path` | **refused** — `bit_width` reads well for the fax bit reader, but `leading_zeros`-based code there is already tested and the change buys style, not correctness. The Cargo items are for CI plumbing we do not have |
| 1.98 | `str::substr_range`; `slice::subslice_range`; `core::fmt::NumBuffer`; `format_into`; `algebraic_*` float operations; `String::from_utf16le`/`be` | **not applicable** — no substring indexing that needs a range, no UTF-16 input (PDF strings are PDFDocEncoding or UTF-16BE *with* a BOM, handled in `src/object.rs`), and float reproducibility is not a claim this renderer makes |
| 1.99 | `IntoIterator` for `Box<[T; N]>`; `VecDeque::retain_back`; `Box::into_non_null`; `Vec::into_parts`/`from_parts`; `String::from_utf8_lossy_owned` | **not applicable** — the one tempting item, `into_parts`, is a raw-parts API the `unsafe_code` denial rules out in spirit |
| 1.100 | never type stabilised; allocator API (`Allocator`, `Global`, `Box::new_in`); `PartialEq<Cow<[U]>>` for `Vec`/`&[T]`; Cargo's own lints and publish-age cooldowns; new build-directory layout | **refused** — the allocator API is the headline and we cannot use it (`pdf2` forbids `unsafe`, and a custom allocator is a decision for a caller, not a library); the rest is irrelevant or already handled |

## What the floor changed in the tree

- **`.clippy.toml`'s `msrv`** is the switch that makes any of this visible: it
  *overrides* `rust-version` for clippy, and with it at 1.87 clippy refused to
  suggest `as_chunks`. It moved with the manifests, and `chunks_exact_to_as_chunks`
  is now denied by the workspace's `clippy::all`, so the old form cannot come back.
- **Edition 2024** is what makes let-chains available at all; the catalogue line for
  1.88 says so, and `AGENTS.md` repeats it where someone will read it before writing
  a nested `if let`.
- **`assert_matches!` is not in the prelude**, which cost one compile error per file
  until each imported it. Worth writing down: the release note says "stabilised",
  not "prelude".

## How a future bump should use this file

Read the range's `RELEASES.md` sections for **Stabilized APIs** and **Language**,
add one line per release here with a verdict, and only then change the number. A
line marked *used* must name the file it landed in; a line marked *refused* must say
what would change the answer.

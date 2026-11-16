# Third-party notices

The crate ships data it did not write. This file is the notice that has to travel
with it.

## Adobe CMap resources

`vendor/adobe-cmaps/` holds two of Adobe's CMap resources, and
`src/data/japan1_90ms_rksj.bin` is generated from them by `cargo xtask cmaps`:

- `90ms-RKSJ-H` — Shift-JIS byte codes to Adobe-Japan1 CIDs.
- `UniJIS-UCS2-H` — UTF-16BE code units to the same CIDs, read backwards so a
  code finds its text.

Both are used under Adobe's BSD-3-Clause licence, reproduced in full in
`vendor/adobe-cmaps/LICENSE.md` and summarised here:

```text
Copyright 1990-2023 Adobe. All rights reserved.

Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.
3. Neither the name of Adobe nor the names of its contributors may be used to
   endorse or promote products derived from this software without specific prior
   written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND
ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED
WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR
ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES
(INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES;
LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON
ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT
(INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

The generated table is a derived work: it contains no Adobe code, only the
code → text pairs the two resources imply, ranked so that an ordinary ideograph
wins over a compatibility variant.

## CCITT fax run-length tables

`src/fax.rs` carries the ITU-T T.4/T.6 white and black run-length code tables as
data. They were transcribed mechanically from [`hayro-ccitt` 0.5.0][hayro]
(Apache-2.0 OR MIT) rather than typed from memory — the facts are the standard's,
the transcription is a courtesy. `hayro-ccitt` itself is **not** a dependency: its
`rust-version` is 1.92, so only the tables travel.

[hayro]: https://crates.io/crates/hayro-ccitt/0.5.0

## pdf.js `src/core/ccitt.js` (Apache-2.0) — structure reference

`src/fax.rs`'s two-dimensional decoder follows the *structure* of
`src/core/ccitt.js` from mozilla/pdf.js, pinned at tag `v1.10.100` (fetched
2026-11-16; Apache-2.0, and the notice above covers the licence). What was taken is
the shape of the algorithm: the coding line as a list of change positions whose
parity carries the run colour (`_addPixels`/`_addPixelsNeg`), the reference
position tracked as `a0` advances, and the order of the mode codes. No code was
copied verbatim — the Rust is written against the same model, and the mode tables
come from the T.4 tables already cited above.

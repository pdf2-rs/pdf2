//! Fonts for text rendering: a provider trait, a built-in base-14 fallback,
//! TrueType outlines through `ttf-parser`, and CFF outlines — CID-keyed or
//! plain — by wrapping the bare CFF program in the smallest OpenType container
//! `ttf-parser` will read.
//!
//! The wrapper exists because PDF carries two shapes of CFF: `/Subtype
//! /OpenType` (already an sfnt, read directly) and `/Subtype /CIDFontType0C` or
//! `/Type1C` (a bare CFF table, which `ttf-parser` refuses: "Invalid sfnt
//! version"). Five synthesised tables — `head`, `hhea`, `maxp`, `hmtx` and the
//! program itself — are enough, and the decision, its measurements and what it
//! refuses are in `docs/adr/0003-cff-without-a-dependency.md`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pdf2::Encoding;

use crate::geom::Point;

/// A font's parsed answers, shared by its clones.
///
/// `ttf-parser` borrows the program, so a parsed face cannot outlive the call
/// that made it — but its answers can. Two maps memoise the expensive steps:
/// character → glyph id (the `cmap` lookup) and glyph id → outline. Measured on
/// `book.pdf` at scale 0.25, where the rasteriser is not the bottleneck, this
/// halves the page: 5.4 ms → 3.9 ms.
#[derive(Debug, Default)]
pub struct Cache {
    /// Character → glyph id, `None` when the `cmap` does not cover it.
    cmap: BTreeMap<char, Option<u16>>,
    /// Glyph id → outline and advance.
    glyphs: BTreeMap<u16, Option<Glyph>>,
}

/// The shared cache behind a font's clones.
pub type GlyphCache = Rc<RefCell<Cache>>;

/// One glyph's outline in em units: y grows upwards, the baseline is y = 0 and
/// one em is 1.0.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Glyph {
    /// Closed contours of the outline.
    pub contours: Vec<Vec<Point>>,
    /// Advance width in em units.
    pub advance: f64,
}

/// A resolved font ready to place glyphs.
#[derive(Debug, Clone)]
pub enum Font {
    /// The built-in fallback: a 5×7 bitmap alphabet, used when the document
    /// carries no usable font program (all of the base-14 cases).
    Builtin {
        /// Code → character table.
        encoding: Encoding,
    },
    /// An embedded CFF program, wrapped so the outlines come out of the same
    /// code path as TrueType. CID-keyed fonts select glyphs through their
    /// charset, not by using the CID as a glyph id.
    Cff {
        /// Code → character table, for the (rare) simple CFF font.
        encoding: Encoding,
        /// The program as sfnt bytes.
        data: Vec<u8>,
        /// CID → glyph id from the CFF charset, empty when the font is not
        /// CID-keyed.
        cid_to_gid: Vec<(u16, u16)>,
        /// A **simple** (non-CID) CFF font selects its glyphs by name: the
        /// charset names each glyph, and a code reaches one through the
        /// encoding's character. A bare CFF carries no `cmap`, so this is the
        /// only route to its outlines.
        names: std::collections::HashMap<char, u16>,
        /// Parsed answers, shared by clones.
        cache: GlyphCache,
    },
    /// An embedded TrueType program.
    TrueType {
        /// Code → character table.
        encoding: Encoding,
        /// The font program, still compressed in the PDF sense is not our
        /// business: this is the decoded `/FontFile2`.
        data: Vec<u8>,
        /// Whether the program carries a `cmap` table at all: when it does not,
        /// a simple font's code is the glyph id.
        has_cmap: bool,
        /// Parsed answers, shared by clones.
        cache: GlyphCache,
    },
}

impl Font {
    /// The built-in fallback with an encoding.
    pub fn builtin(encoding: Encoding) -> Self {
        Self::Builtin { encoding }
    }

    /// An embedded CFF program, bare or already wrapped in OpenType.
    ///
    /// `/CIDFontType0C` and `/Type1C` programs are a CFF table with no container;
    /// they are wrapped here so the same outline code reads them.
    pub fn cff(encoding: Encoding, data: Vec<u8>) -> Option<Self> {
        let wrapped = data.starts_with(b"OTTO");
        let table: Vec<u8> = if wrapped {
            cff_table_of_sfnt(&data)?.to_vec()
        } else {
            data.clone()
        };
        let (charset_at, glyphs) = cff_layout(&table)?;
        // A CID-keyed font's Top DICT declares an ROS; without one the charset
        // holds *names* (SIDs), which is what a simple font selects by.
        let cid_to_gid = if top_dict_declares_ros(&table) {
            cff_charset(&table, charset_at, glyphs)
        } else {
            Vec::new()
        };
        let program = if wrapped {
            data
        } else {
            wrap_cff(&table, glyphs)?
        };
        // Parse once here so a program that cannot be read is rejected at load,
        // not at the first glyph.
        let face = ttf_parser::Face::parse(&program, 0).ok()?;
        let names = if cid_to_gid.is_empty() {
            glyph_names(&face)
        } else {
            std::collections::HashMap::new()
        };
        Some(Self::Cff {
            encoding,
            data: program,
            cid_to_gid,
            names,
            cache: GlyphCache::default(),
        })
    }

    /// An embedded TrueType program, rejected when it cannot be parsed.
    pub fn true_type(encoding: Encoding, data: Vec<u8>) -> Option<Self> {
        let face = ttf_parser::Face::parse(&data, 0).ok()?;
        if face.units_per_em() == 0 {
            return None;
        }
        let has_cmap = face.tables().cmap.is_some();
        Some(Self::TrueType {
            encoding,
            data,
            has_cmap,
            cache: GlyphCache::default(),
        })
    }

    /// The outline and advance for a byte code, or `None` when there is no
    /// glyph for it.
    ///
    /// A simple font's code goes through its encoding to a character, and the
    /// character through the program's `cmap` to a glyph id. A program with **no
    /// `cmap` table at all** cannot be consulted that way, and the files that
    /// lack one mean the code *is* the glyph id — pdf.js's
    /// `TrueType_without_cmap.pdf` is the case, and refusing to guess drew a
    /// blank page while reporting nothing at all.
    pub fn glyph(&self, code: u8) -> Option<Glyph> {
        match self {
            Self::Builtin { encoding } => builtin_glyph(encoding.character(code)?),
            Self::TrueType {
                encoding,
                data,
                cache,
                ..
            } => {
                let character = encoding.character(code)?;
                cached_glyph(data, cache, glyph_id(data, cache, character)?)
            }
            Self::Cff {
                encoding,
                data,
                names,
                cache,
                ..
            } => {
                let character = encoding.character(code)?;
                // A simple (non-CID) CFF font reaches its glyphs through the
                // charset's names; a CID-keyed one through the CID it is asked
                // for, which is the composite path.
                let id = if names.is_empty() {
                    glyph_id(data, cache, character)?
                } else {
                    ttf_parser::GlyphId(*names.get(&character)?)
                };
                cached_glyph(data, cache, id)
            }
        }
    }

    /// Whether glyphs are selectable at all — the name map of a simple CFF, a
    /// `cmap` of a TrueType program.
    pub fn has_glyph_names(&self) -> bool {
        match self {
            Self::Cff { names, .. } => !names.is_empty(),
            Self::TrueType { has_cmap, .. } => *has_cmap,
            Self::Builtin { .. } => false,
        }
    }

    /// Whether this font has a character map at all.
    ///
    /// A simple TrueType program with none cannot be decoded from its codes, so
    /// the caller reports the page instead of drawing an empty one.
    pub fn has_character_map(&self) -> bool {
        match self {
            Self::Builtin { .. } | Self::Cff { .. } => true,
            Self::TrueType { has_cmap, .. } => *has_cmap,
        }
    }

    /// The advance of a code in em units, falling back to the built-in metric.
    pub fn advance(&self, code: u8) -> f64 {
        self.glyph(code)
            .map_or(BUILTIN_ADVANCE, |glyph| glyph.advance)
    }

    /// The outline for a glyph id, which is how composite (Type 0) fonts select
    /// glyphs: a CID maps to an id through `/CIDToGIDMap`, not through a cmap.
    ///
    /// The built-in bitmap alphabet has no glyph ids, so it has no answer here.
    pub fn glyph_by_id(&self, id: u16) -> Option<Glyph> {
        match self {
            Self::Builtin { .. } => None,
            Self::TrueType { data, cache, .. } => {
                cached_glyph(data, cache, ttf_parser::GlyphId(id))
            }
            Self::Cff {
                data,
                cid_to_gid,
                cache,
                ..
            } => {
                // In a CID-keyed CFF the charset maps glyph ids to CIDs, so a
                // CID has to be looked up rather than used as the glyph id.
                let glyph = cid_to_gid
                    .iter()
                    .find(|(cid, _)| *cid == id)
                    .map_or(id, |(_, gid)| *gid);
                cached_glyph(data, cache, ttf_parser::GlyphId(glyph))
            }
        }
    }
}

/// The character each glyph of a simple CFF font is named for.
///
/// A name that is not a character name (`/uni20AC` style is handled by the
/// library's table) simply does not appear, and the code that would have
/// selected it draws nothing rather than a wrong glyph.
fn glyph_names(face: &ttf_parser::Face<'_>) -> std::collections::HashMap<char, u16> {
    let mut map = std::collections::HashMap::new();
    for id in 0..face.number_of_glyphs() {
        let Some(name) = face.glyph_name(ttf_parser::GlyphId(id)) else {
            continue;
        };
        if let Some(character) = pdf2::encoding::glyph_name_character(name.as_bytes()) {
            map.entry(character).or_insert(id);
        }
    }
    map
}

/// A character's glyph id, memoised: the `cmap` lookup needs a parsed face.
fn glyph_id(data: &[u8], cache: &GlyphCache, character: char) -> Option<ttf_parser::GlyphId> {
    if let Some(cached) = cache.borrow().cmap.get(&character) {
        return cached.map(ttf_parser::GlyphId);
    }
    let face = ttf_parser::Face::parse(data, 0).ok()?;
    let found = face.glyph_index(character).map(|id| id.0);
    cache.borrow_mut().cmap.insert(character, found);
    found.map(ttf_parser::GlyphId)
}

/// An outline and advance, memoised: the parse is the expensive part.
fn cached_glyph(data: &[u8], cache: &GlyphCache, id: ttf_parser::GlyphId) -> Option<Glyph> {
    if let Some(cached) = cache.borrow().glyphs.get(&id.0) {
        return cached.clone();
    }
    let glyph = glyph_from_face(data, id);
    cache.borrow_mut().glyphs.insert(id.0, glyph.clone());
    glyph
}

/// Wrap a bare CFF table in the smallest OpenType container `ttf-parser` reads.
///
/// The synthesised tables say only what outlines need: 1000 units per em (CFF's
/// default `/FontMatrix` is 0.001), the glyph count from the CFF `CharStrings`
/// INDEX, and zero advances, because a CID font's advances come from `/W` in the
/// PDF. Checksums are left zero: `ttf-parser` does not read them, and inventing
/// them would be a claim nobody checks.
fn wrap_cff(cff: &[u8], glyphs: u16) -> Option<Vec<u8>> {
    let mut head = [0_u8; 54];
    head[0..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    head[4..8].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    head[12..16].copy_from_slice(&0x5F0F_3CF5_u32.to_be_bytes());
    head[18..20].copy_from_slice(&1000_u16.to_be_bytes());
    let mut hhea = [0_u8; 36];
    hhea[0..4].copy_from_slice(&0x0001_0000_u32.to_be_bytes());
    hhea[34..36].copy_from_slice(&glyphs.to_be_bytes());
    let mut maxp = [0_u8; 6];
    maxp[0..4].copy_from_slice(&0x0000_5000_u32.to_be_bytes());
    maxp[4..6].copy_from_slice(&glyphs.to_be_bytes());
    let hmtx = vec![0_u8; usize::from(glyphs) * 4];

    let tables: [(&[u8; 4], &[u8]); 5] = [
        (b"CFF ", cff),
        (b"head", &head),
        (b"hhea", &hhea),
        (b"hmtx", &hmtx),
        (b"maxp", &maxp),
    ];
    let count = u16::try_from(tables.len()).ok()?;
    let mut out = Vec::with_capacity(cff.len() + 256);
    out.extend_from_slice(&0x4F54_544F_u32.to_be_bytes());
    out.extend_from_slice(&count.to_be_bytes());
    let pow2 = 1_u32 << u32::from(count).ilog2();
    #[allow(clippy::cast_possible_truncation)]
    let search_range = pow2 * 16;
    out.extend_from_slice(&u16::try_from(search_range).ok()?.to_be_bytes());
    #[allow(clippy::cast_possible_truncation)]
    let entry_selector = count.trailing_zeros() as u16;
    out.extend_from_slice(&entry_selector.to_be_bytes());
    #[allow(clippy::cast_possible_truncation)]
    let range_shift = (u32::from(count) * 16).saturating_sub(search_range) as u16;
    out.extend_from_slice(&range_shift.to_be_bytes());

    let mut offset = 12 + usize::from(count) * 16;
    let mut body: Vec<u8> = Vec::with_capacity(cff.len() + 128);
    for (tag, data) in &tables {
        let mut table = data.to_vec();
        while !table.len().is_multiple_of(4) {
            table.push(0);
        }
        out.extend_from_slice(*tag);
        out.extend_from_slice(&0_u32.to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).ok()?.to_be_bytes());
        offset += table.len();
        body.extend_from_slice(&table);
    }
    out.extend_from_slice(&body);
    Some(out)
}

/// The `CFF ` table inside an OpenType file.
fn cff_table_of_sfnt(data: &[u8]) -> Option<&[u8]> {
    let count = usize::from(u16::from_be_bytes([*data.get(4)?, *data.get(5)?]));
    for index in 0..count {
        let at = 12 + index * 16;
        if data.get(at..at + 4)? == b"CFF " {
            let offset = u32::from_be_bytes([
                *data.get(at + 8)?,
                *data.get(at + 9)?,
                *data.get(at + 10)?,
                *data.get(at + 11)?,
            ]);
            let length = u32::from_be_bytes([
                *data.get(at + 12)?,
                *data.get(at + 13)?,
                *data.get(at + 14)?,
                *data.get(at + 15)?,
            ]);
            let start = usize::try_from(offset).ok()?;
            let end = start.checked_add(usize::try_from(length).ok()?)?;
            return data.get(start..end);
        }
    }
    None
}

/// A CFF table's charset offset and glyph count.
fn cff_layout(cff: &[u8]) -> Option<(usize, u16)> {
    let header_size = usize::from(*cff.get(2)?);
    let (_, after_name) = cff_index(cff, header_size)?;
    let (top_count, after_top) = cff_index(cff, after_name)?;
    if top_count == 0 {
        return None;
    }
    let (_, after_strings) = cff_index(cff, after_top)?;
    let (_, after_global_subrs) = cff_index(cff, after_strings)?;
    let _ = after_global_subrs;
    let top_dict = cff_index_item(cff, after_name, 0)?;
    let charstrings_at = top_dict_operator(top_dict, 17)?;
    let (glyphs, _) = cff_index(cff, charstrings_at)?;
    let charset_at = top_dict_operator(top_dict, 15).unwrap_or(0);
    Some((charset_at, u16::try_from(glyphs).ok()?))
}

/// Whether the Top DICT declares an ROS (registry, ordering, supplement), which
/// is what makes a CFF font CID-keyed (`9.7.4` of the CFF spec).
fn top_dict_declares_ros(cff: &[u8]) -> bool {
    let Some(header_size) = cff.get(2).map(|size| usize::from(*size)) else {
        return false;
    };
    let Some((_, after_name)) = cff_index(cff, header_size) else {
        return false;
    };
    let Some(dict) = cff_index_item(cff, after_name, 0) else {
        return false;
    };
    // Operator 12 30 (ROS): operands, then the two bytes, in that order.
    dict.windows(2).any(|window| window == [12, 30])
}

/// The CID → glyph id pairs a CID-keyed CFF charset declares.
///
/// Formats 0, 1 and 2 are read; the three predefined charsets (offsets 0, 1 and
/// 2) are the standard Latin character sets, which are not CID-keyed, so they
/// yield no pairs and the CID is used as the glyph id.
fn cff_charset(cff: &[u8], at: usize, glyphs: u16) -> Vec<(u16, u16)> {
    let mut pairs = Vec::new();
    if at < 3 {
        return pairs;
    }
    let Some(&format) = cff.get(at) else {
        return pairs;
    };
    let mut index = at + 1;
    let mut gid = 1u16;
    match format {
        0 => {
            while gid < glyphs {
                let cid = cff
                    .get(index..index + 2)
                    .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
                let Some(cid) = cid else { break };
                pairs.push((cid, gid));
                gid += 1;
                index += 2;
            }
        }
        1 | 2 => {
            while gid < glyphs {
                let width = if format == 1 { 1 } else { 2 };
                let first = cff
                    .get(index..index + 2)
                    .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]));
                let left = cff.get(index + 2..index + 2 + width).map(|bytes| {
                    if width == 1 {
                        u16::from(bytes[0])
                    } else {
                        u16::from_be_bytes([bytes[0], bytes[1]])
                    }
                });
                let (Some(first), Some(left)) = (first, left) else {
                    break;
                };
                for offset in 0..=left {
                    if gid >= glyphs {
                        break;
                    }
                    pairs.push((first.wrapping_add(offset), gid));
                    gid += 1;
                }
                index += 2 + width;
            }
        }
        _ => {}
    }
    pairs
}

/// A CFF INDEX: its entry count and the offset just past it.
fn cff_index(data: &[u8], at: usize) -> Option<(usize, usize)> {
    let count = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
    if count == 0 {
        return Some((0, at + 2));
    }
    let off_size = usize::from(*data.get(at + 2)?);
    if !(1..=4).contains(&off_size) {
        return None;
    }
    let offsets_at = at + 3;
    let data_at = offsets_at + (count + 1) * off_size;
    let mut last = 0;
    for index in 0..off_size {
        last = (last << 8) | usize::from(*data.get(offsets_at + count * off_size + index)?);
    }
    Some((count, data_at + last.saturating_sub(1)))
}

/// One entry of a CFF INDEX, by position.
fn cff_index_item(data: &[u8], at: usize, item: usize) -> Option<&[u8]> {
    let count = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
    if item >= count {
        return None;
    }
    let off_size = usize::from(*data.get(at + 2)?);
    let offsets_at = at + 3;
    let data_at = offsets_at + (count + 1) * off_size;
    let read = |position: usize| -> Option<usize> {
        let base = offsets_at + position * off_size;
        let mut value = 0;
        for index in 0..off_size {
            value = (value << 8) | usize::from(*data.get(base + index)?);
        }
        Some(value)
    };
    let start = read(item)?;
    let end = read(item + 1)?;
    data.get(data_at + start - 1..data_at + end - 1)
}

/// The last operand of `operator` inside a CFF Top DICT.
fn top_dict_operator(dict: &[u8], operator: u8) -> Option<usize> {
    let mut operands: Vec<f64> = Vec::new();
    let mut index = 0;
    while index < dict.len() {
        let byte = *dict.get(index)?;
        index += 1;
        match byte {
            28 => {
                let value = i16::from_be_bytes([*dict.get(index)?, *dict.get(index + 1)?]);
                operands.push(f64::from(value));
                index += 2;
            }
            29 => {
                let mut bytes = [0_u8; 4];
                for (offset, slot) in bytes.iter_mut().enumerate() {
                    *slot = *dict.get(index + offset)?;
                }
                operands.push(f64::from(i32::from_be_bytes(bytes)));
                index += 4;
            }
            32..=246 => operands.push(f64::from(byte) - 139.0),
            247..=250 => {
                let next = f64::from(*dict.get(index)?);
                operands.push((f64::from(byte) - 247.0) * 256.0 + next + 108.0);
                index += 1;
            }
            251..=254 => {
                let next = f64::from(*dict.get(index)?);
                operands.push(-(f64::from(byte) - 251.0) * 256.0 - next - 108.0);
                index += 1;
            }
            30 => {
                // A real number; no Top DICT operand this module reads needs
                // one, so it is consumed and ignored.
                while index < dict.len() {
                    let nibble = dict[index];
                    index += 1;
                    if nibble & 0x0F == 0x0F || nibble >> 4 == 0x0F {
                        break;
                    }
                }
                operands.push(f64::NAN);
            }
            0..=21 => {
                if byte == 12 {
                    index += 1;
                }
                if byte == operator {
                    if let Some(value) = operands.last() {
                        if value.is_finite() && *value >= 0.0 {
                            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                            return Some(*value as usize);
                        }
                    }
                    return None;
                }
                operands.clear();
            }
            _ => operands.clear(),
        }
    }
    None
}

/// An outline for a glyph id, from a parsed TrueType program.
fn glyph_from_face(data: &[u8], glyph_id: ttf_parser::GlyphId) -> Option<Glyph> {
    let face = ttf_parser::Face::parse(data, 0).ok()?;
    let units = f64::from(face.units_per_em());
    let mut builder = OutlineBuilder::new(units);
    face.outline_glyph(glyph_id, &mut builder)?;
    let advance = f64::from(face.glyph_hor_advance(glyph_id).unwrap_or(0)) / units;
    Some(Glyph {
        contours: builder.contours,
        advance,
    })
}

const BUILTIN_ADVANCE: f64 = 6.0 / 9.0;
const BUILTIN_CELL: f64 = 1.0 / 9.0;
const BUILTIN_WIDTH: u8 = 5;
const BUILTIN_HEIGHT: u8 = 7;

/// A 5×7 bitmap alphabet. Lowercase letters use the uppercase shapes; anything
/// without a pattern is drawn as a notdef box.
fn builtin_glyph(character: char) -> Option<Glyph> {
    let code = u8::try_from(u32::from(character)).ok()?;
    if character == ' ' {
        return Some(Glyph {
            contours: Vec::new(),
            advance: BUILTIN_ADVANCE,
        });
    }
    if let Some(rows) = glyph_rows(code) {
        return Some(Glyph {
            contours: rows_to_contours(&rows),
            advance: BUILTIN_ADVANCE,
        });
    }
    if character == '\u{2022}' {
        return Some(Glyph {
            contours: rows_to_contours(&[0, 0, 0b00100, 0b01110, 0b00100, 0, 0]),
            advance: BUILTIN_ADVANCE,
        });
    }
    // Latin-1 text is the common case for base-14 fonts, so an accented letter
    // is drawn as its unaccented base plus the mark rather than as .notdef.
    if let Some((base, accent)) = decompose(character) {
        let rows = glyph_rows(u8::try_from(u32::from(base)).unwrap_or(b'?'))
            .or_else(|| upper(base).and_then(|upper| glyph_rows(upper as u8)))
            .unwrap_or_else(notdef_rows);
        let mut contours = rows_to_contours(&rows);
        contours.extend(
            accent
                .cells()
                .iter()
                .map(|&(column, layer)| mark_cell(column, layer)),
        );
        return Some(Glyph {
            contours,
            advance: BUILTIN_ADVANCE,
        });
    }
    if character == '\u{00F8}' || character == '\u{00D8}' {
        // `ø` is `o` with a slash through it, which no mark above the box can
        // show, so the slash goes across the glyph body instead.
        let mut contours = rows_to_contours(&glyph_rows(b'O').unwrap_or_else(notdef_rows));
        contours.extend(
            [(0, 5), (1, 4), (2, 3), (3, 2), (4, 1)]
                .iter()
                .map(|&(column, layer)| mark_cell(column, layer)),
        );
        return Some(Glyph {
            contours,
            advance: BUILTIN_ADVANCE,
        });
    }
    let rows = upper(character)
        .and_then(|upper| glyph_rows(upper as u8))
        .unwrap_or_else(notdef_rows);
    Some(Glyph {
        contours: rows_to_contours(&rows),
        advance: BUILTIN_ADVANCE,
    })
}

/// A mark that can sit on a 5×7 base letter: above the glyph box (layers 0 and
/// up) or below the baseline (negative layers, used by the cedilla).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Accent {
    Acute,
    Grave,
    Circumflex,
    Diaeresis,
    Tilde,
    Ring,
    Cedilla,
}

impl Accent {
    /// Cell coordinates: `column` across the five-column box, `layer` up from
    /// the top of the glyph (layer 0 starts at 7/9 em) or, when negative, down
    /// from the baseline.
    fn cells(self) -> &'static [(u8, i8)] {
        match self {
            Self::Acute => &[(2, 0), (3, 0), (3, 1)],
            Self::Grave => &[(2, 1), (3, 1), (3, 0)],
            Self::Circumflex => &[(1, 0), (2, 1), (3, 0)],
            Self::Diaeresis => &[(1, 1), (3, 1)],
            Self::Tilde => &[(1, 1), (2, 0), (3, 1)],
            Self::Ring => &[(1, 0), (2, 1), (3, 0), (2, 0)],
            Self::Cedilla => &[(2, -1), (3, -1), (3, -2)],
        }
    }
}

/// One cell of a mark, as a rectangle in em units.
fn mark_cell(column: u8, layer: i8) -> Vec<Point> {
    let x0 = f64::from(column) * BUILTIN_CELL;
    let x1 = x0 + BUILTIN_CELL;
    let (bottom, top) = if layer < 0 {
        (
            f64::from(layer) * BUILTIN_CELL,
            f64::from(layer + 1) * BUILTIN_CELL,
        )
    } else {
        (
            f64::from(BUILTIN_HEIGHT + layer as u8) * BUILTIN_CELL,
            f64::from(BUILTIN_HEIGHT + layer as u8 + 1) * BUILTIN_CELL,
        )
    };
    vec![
        Point::new(x0, bottom),
        Point::new(x1, bottom),
        Point::new(x1, top),
        Point::new(x0, top),
    ]
}

/// The base letter and mark for a precomposed Latin-1 / Latin Extended-A
/// character the 5×7 alphabet does not carry itself.
fn decompose(character: char) -> Option<(char, Accent)> {
    let (base, accent) = match character {
        'á' | 'Á' => ('a', Accent::Acute),
        'é' | 'É' => ('e', Accent::Acute),
        'í' | 'Í' => ('i', Accent::Acute),
        'ó' | 'Ó' => ('o', Accent::Acute),
        'ú' | 'Ú' => ('u', Accent::Acute),
        'ý' | 'Ý' => ('y', Accent::Acute),
        'à' | 'À' => ('a', Accent::Grave),
        'è' | 'È' => ('e', Accent::Grave),
        'ì' | 'Ì' => ('i', Accent::Grave),
        'ò' | 'Ò' => ('o', Accent::Grave),
        'ù' | 'Ù' => ('u', Accent::Grave),
        'â' | 'Â' => ('a', Accent::Circumflex),
        'ê' | 'Ê' => ('e', Accent::Circumflex),
        'î' | 'Î' => ('i', Accent::Circumflex),
        'ô' | 'Ô' => ('o', Accent::Circumflex),
        'û' | 'Û' => ('u', Accent::Circumflex),
        'ä' | 'Ä' => ('a', Accent::Diaeresis),
        'ë' | 'Ë' => ('e', Accent::Diaeresis),
        'ï' | 'Ï' => ('i', Accent::Diaeresis),
        'ö' | 'Ö' => ('o', Accent::Diaeresis),
        'ü' | 'Ü' => ('u', Accent::Diaeresis),
        'ÿ' | 'Ÿ' => ('y', Accent::Diaeresis),
        'ã' | 'Ã' => ('a', Accent::Tilde),
        'õ' | 'Õ' => ('o', Accent::Tilde),
        'ñ' | 'Ñ' => ('n', Accent::Tilde),
        'å' | 'Å' => ('a', Accent::Ring),
        'ç' | 'Ç' => ('c', Accent::Cedilla),
        _ => return None,
    };
    Some((base, accent))
}

fn upper(character: char) -> Option<char> {
    character
        .to_uppercase()
        .next()
        .filter(|upper| *upper != character)
}

fn notdef_rows() -> [u8; BUILTIN_HEIGHT as usize] {
    [
        0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
    ]
}

fn rows_to_contours(rows: &[u8; BUILTIN_HEIGHT as usize]) -> Vec<Vec<Point>> {
    let mut contours = Vec::new();
    for (row, bits) in rows.iter().enumerate() {
        for column in 0..BUILTIN_WIDTH {
            let mask = 1_u8 << (BUILTIN_WIDTH - 1 - column);
            if bits & mask == 0 {
                continue;
            }
            let x0 = f64::from(column) * BUILTIN_CELL;
            let x1 = x0 + BUILTIN_CELL;
            let top = f64::from(BUILTIN_HEIGHT - 1 - row as u8) * BUILTIN_CELL;
            let bottom = top + BUILTIN_CELL;
            contours.push(vec![
                Point::new(x0, bottom),
                Point::new(x1, bottom),
                Point::new(x1, top),
                Point::new(x0, top),
            ]);
        }
    }
    contours
}

fn glyph_rows(code: u8) -> Option<[u8; 7]> {
    let rows = match code {
        b'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        b'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        b'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        b'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        b'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        b'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        b'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        b'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        b'4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        b'5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        b'6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        b'7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b'9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        b'.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        b',' => [0, 0, 0, 0, 0b01100, 0b01100, 0b01000],
        b'!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        b'?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        b'-' => [0, 0, 0, 0b11111, 0, 0, 0],
        b':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        b';' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0b01000],
        b'\'' => [0b00100, 0b00100, 0b01000, 0, 0, 0, 0],
        b'"' => [0b01010, 0b01010, 0, 0, 0, 0, 0],
        b'(' => [
            0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010,
        ],
        b')' => [
            0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000,
        ],
        b'/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        b'+' => [0, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0],
        b'=' => [0, 0, 0b11111, 0, 0b11111, 0, 0],
        b'*' => [0, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0],
        b'#' => [
            0b01010, 0b01010, 0b11111, 0b01010, 0b11111, 0b01010, 0b01010,
        ],
        b'%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        b'&' => [
            0b01100, 0b10010, 0b10100, 0b01000, 0b10101, 0b10010, 0b01101,
        ],
        _ => return None,
    };
    Some(rows)
}

/// Collects `ttf-parser` outlines into em-space contours.
struct OutlineBuilder {
    units: f64,
    contours: Vec<Vec<Point>>,
    current: Vec<Point>,
}

impl OutlineBuilder {
    fn new(units: f64) -> Self {
        Self {
            units,
            contours: Vec::new(),
            current: Vec::new(),
        }
    }

    fn point(&self, x: f32, y: f32) -> Point {
        Point::new(f64::from(x) / self.units, f64::from(y) / self.units)
    }

    fn finish_contour(&mut self) {
        if self.current.len() > 1 {
            self.contours.push(std::mem::take(&mut self.current));
        }
    }
}

impl ttf_parser::OutlineBuilder for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish_contour();
        self.current.push(self.point(x, y));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.current.push(self.point(x, y));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let Some(&start) = self.current.last() else {
            return;
        };
        let control = self.point(x1, y1);
        let end = self.point(x, y);
        for step in 1..=8 {
            let t = f64::from(step) / 8.0;
            let u = 1.0 - t;
            self.current.push(Point::new(
                u * u * start.x + 2.0 * u * t * control.x + t * t * end.x,
                u * u * start.y + 2.0 * u * t * control.y + t * t * end.y,
            ));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let Some(&start) = self.current.last() else {
            return;
        };
        let c1 = self.point(x1, y1);
        let c2 = self.point(x2, y2);
        let end = self.point(x, y);
        for step in 1..=12 {
            let t = f64::from(step) / 12.0;
            let u = 1.0 - t;
            self.current.push(Point::new(
                u * u * u * start.x
                    + 3.0 * u * u * t * c1.x
                    + 3.0 * u * t * t * c2.x
                    + t * t * t * end.x,
                u * u * u * start.y
                    + 3.0 * u * u * t * c1.y
                    + 3.0 * u * t * t * c2.y
                    + t * t * t * end.y,
            ));
        }
    }

    fn close(&mut self) {
        self.finish_contour();
    }
}

#[cfg(test)]
mod tests {
    use super::Font;
    use pdf2::Encoding;

    #[test]
    fn the_builtin_font_draws_letters_as_ink() {
        let font = Font::builtin(Encoding::standard());
        let glyph = font.glyph(b'H').expect("H");
        assert!(!glyph.contours.is_empty());
        assert!((glyph.advance - 0.6667).abs() < 1e-3);
        // Every contour is a rectangle inside the glyph box.
        for contour in &glyph.contours {
            assert_eq!(contour.len(), 4);
            for point in contour {
                assert!(point.x >= 0.0 && point.x <= 6.0 / 9.0 + 1e-9);
                assert!(point.y >= 0.0 && point.y <= 7.0 / 9.0 + 1e-9);
            }
        }
    }

    #[test]
    fn the_builtin_font_is_blank_for_space() {
        let font = Font::builtin(Encoding::standard());
        let glyph = font.glyph(b' ').expect("space");
        assert!(glyph.contours.is_empty());
        assert!(glyph.advance > 0.0);
    }

    #[test]
    fn glyph_ids_are_selected_directly_for_composite_fonts() {
        let font = Font::builtin(Encoding::standard());
        assert!(
            font.glyph_by_id(1).is_none(),
            "the bitmap alphabet has no glyph ids"
        );
    }

    #[test]
    fn a_broken_font_program_is_rejected() {
        assert!(Font::true_type(Encoding::standard(), vec![0, 1, 2, 3]).is_none());
        assert!(Font::true_type(Encoding::standard(), Vec::new()).is_none());
    }

    #[test]
    fn accented_latin_composes_instead_of_drawing_notdef() {
        let font = Font::builtin(Encoding::win_ansi());
        // code, the ASCII base the accent is composed onto, and the number of
        // mark cells the accent contributes.
        for (code, base, marks) in [
            (0xE9_u8, b'E', 3), // é, acute
            (0xF1, b'N', 3),    // ñ, tilde
            (0xFC, b'U', 2),    // ü, diaeresis
            (0xE7, b'C', 3),    // ç, cedilla
            (0xE5, b'A', 4),    // å, ring
            (0xE2, b'A', 3),    // â, circumflex
        ] {
            let glyph = font.glyph(code).expect("a composed glyph");
            let plain = font.glyph(base).expect("the base letter");
            assert_eq!(
                glyph.contours.len(),
                plain.contours.len() + marks,
                "{} on {}",
                code as char,
                base as char
            );
            assert_ne!(glyph.contours, plain.contours);
        }
        // Anything the table does not name still falls back to .notdef, and
        // that is a hollow box, not a panic.
        let other = font.glyph(0xA4).expect("undefined code");
        assert_eq!(other.contours.len(), 20, "the notdef box");
    }

    #[test]
    fn marks_sit_outside_the_glyph_box_and_cedillas_below_the_baseline() {
        let font = Font::builtin(Encoding::win_ansi());
        let acute = font.glyph(0xE9).expect("é");
        let top = acute
            .contours
            .iter()
            .flatten()
            .fold(f64::MIN, |max, point| max.max(point.y));
        assert!((7.0 / 9.0..=1.0).contains(&top), "accent tops out at {top}");
        let cedilla = font.glyph(0xE7).expect("ç");
        let bottom = cedilla
            .contours
            .iter()
            .flatten()
            .fold(f64::MAX, |min, point| min.min(point.y));
        assert!(bottom < 0.0, "the cedilla hangs to {bottom}");
    }

    #[test]
    fn unknown_characters_become_the_notdef_box() {
        let font = Font::builtin(Encoding::standard());
        // 0xA4 is undefined in StandardEncoding, so there is no glyph at all.
        assert!(font.glyph(0xA4).is_none());
        // A mapped but unpatterned character gets the notdef box, not a panic.
        let glyph = font.glyph(b'@').expect("@");
        assert_eq!(
            glyph.contours.len(),
            20,
            "the notdef box is a hollow 5×7 frame"
        );
    }
}

#[cfg(test)]
mod cff_tests {
    use super::{cff_layout, top_dict_operator, wrap_cff, Font};

    /// A three-glyph bare CFF built by hand: header, one-entry Name INDEX, a Top
    /// DICT pointing at its CharStrings, empty String and Global Subr INDEXes,
    /// and three empty charstrings (`endchar`).
    fn synthetic_cff() -> Vec<u8> {
        let mut cff = vec![1, 0, 4, 4];
        // Name INDEX: one entry, "A".
        cff.extend_from_slice(&[0, 1, 1, 1, 2, b'A']);
        // Top DICT INDEX: one entry of 6 bytes, CharStrings at offset 25.
        cff.extend_from_slice(&[0, 1, 1, 1, 7]);
        cff.extend_from_slice(&[29, 0, 0, 0, 25, 17]);
        // String INDEX and Global Subr INDEX: both empty.
        cff.extend_from_slice(&[0, 0]);
        cff.extend_from_slice(&[0, 0]);
        assert_eq!(cff.len(), 25, "the CharStrings offset above");
        // CharStrings INDEX: three entries, all empty.
        cff.extend_from_slice(&[0, 3, 1, 1, 1, 1, 1]);
        cff
    }

    #[test]
    fn a_bare_cff_becomes_a_face_ttf_parser_reads() {
        let cff = synthetic_cff();
        let (charset_at, glyphs) = cff_layout(&cff).expect("layout");
        assert_eq!((charset_at, glyphs), (0, 3), "predefined charset, 3 glyphs");
        let wrapped = wrap_cff(&cff, glyphs).expect("wrap");
        let face = ttf_parser::Face::parse(&wrapped, 0).expect("parse the wrapper");
        assert_eq!(face.number_of_glyphs(), 3);
        assert_eq!(face.units_per_em(), 1000);
        assert!(Font::cff(pdf2::Encoding::standard(), cff).is_some());
    }

    #[test]
    fn a_program_without_charstrings_is_refused() {
        // The same file with its Top DICT truncated: no CharStrings operator.
        let cff = synthetic_cff();
        assert!(cff_layout(&cff[..8]).is_none());
        assert!(Font::cff(pdf2::Encoding::standard(), vec![0, 1]).is_none());
    }

    #[test]
    fn the_top_dict_operator_reader_handles_each_number_encoding() {
        // 29 = 32-bit integer, 28 = 16-bit, 32..=246 = byte - 139.
        assert_eq!(top_dict_operator(&[29, 0, 0, 1, 0, 17], 17), Some(256));
        assert_eq!(top_dict_operator(&[28, 1, 0, 17], 17), Some(256));
        assert_eq!(top_dict_operator(&[139 + 42, 17], 17), Some(42));
        // 251..=254 encode negative numbers, which no offset may be.
        assert_eq!(top_dict_operator(&[251, 0, 17], 17), None);
        assert_eq!(
            top_dict_operator(&[29, 0, 0, 0, 9], 17),
            None,
            "no operator"
        );
    }
}

#[cfg(test)]
mod cache_tests {
    use super::Font;
    use pdf2::Encoding;

    fn subset_font() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/fonts/CrimsonText-Regular-subset.ttf"
        ))
        .expect("the committed subset font")
    }

    #[test]
    fn a_glyph_looked_up_twice_is_the_same_outline() {
        let font = Font::true_type(Encoding::standard(), subset_font()).expect("font");
        let first = font.glyph(b'A').expect("A");
        let second = font.glyph(b'A').expect("A");
        assert_eq!(first, second, "the cache returns what the parser did");
        assert!(!first.contours.is_empty(), "A is a drawn glyph");
        assert!(first.advance > 0.0);
        // A different code is a different glyph, not a cache collision.
        let b = font.glyph(b'B').expect("B");
        assert_ne!(first, b);
    }

    #[test]
    fn the_cache_is_shared_by_clones_and_survives_them() {
        let font = Font::true_type(Encoding::standard(), subset_font()).expect("font");
        let clone = font.clone();
        let from_original = font.glyph(b'C').expect("C");
        let from_clone = clone.glyph(b'C').expect("C");
        assert_eq!(from_original, from_clone);
    }

    #[test]
    fn a_program_with_a_cmap_answers_the_question_honestly() {
        let with_cmap = Font::true_type(Encoding::standard(), subset_font()).expect("font");
        assert!(with_cmap.has_character_map());
        assert!(with_cmap.glyph(b'Z').is_some(), "the subset carries Z");
        assert!(
            with_cmap.glyph(0x00).is_none(),
            "an unmapped code has no glyph"
        );
    }
}

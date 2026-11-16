//! Stream filters (§7.4): Flate, LZW, ASCIIHex, ASCII85, RunLength and the
//! predictor functions that Flate/LZW payloads may be wrapped in.
//!
//! Decoding is pure: [`decode_chain`] takes bytes and returns bytes, so a
//! filter can be tested without a document. Resolving indirect `/Filter`,
//! `/DecodeParms` or `/Length` entries is [`crate::Document`]'s job.

use alloc::vec;
use alloc::vec::Vec;

use miniz_oxide::inflate::stream::{inflate, InflateState};
use miniz_oxide::{DataFormat, MZFlush, MZStatus};

use crate::error::{Error, Result};
use crate::lexer::{hex_value, is_whitespace};
use crate::object::{Dictionary, Object, Stream};

/// A filter named by `/Filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    /// `/FlateDecode` — zlib-wrapped DEFLATE.
    FlateDecode,
    /// `/LZWDecode` — LZW with variable code width.
    LzwDecode,
    /// `/ASCIIHexDecode` — hex digits, `>` terminates.
    AsciiHexDecode,
    /// `/ASCII85Decode` — base-85, `~>` terminates.
    Ascii85Decode,
    /// `/RunLengthDecode` — literal and repeat runs.
    RunLengthDecode,
    /// `/CCITTFaxDecode` — Group 3 or 4 fax, one bit per sample.
    CcittFaxDecode,
}

impl Filter {
    /// Map a `/Filter` name to a filter.
    pub fn from_name(name: &[u8]) -> Result<Self> {
        match name {
            b"FlateDecode" | b"Fl" => Ok(Self::FlateDecode),
            b"LZWDecode" | b"LZW" => Ok(Self::LzwDecode),
            b"ASCIIHexDecode" | b"AHx" => Ok(Self::AsciiHexDecode),
            b"ASCII85Decode" | b"A85" => Ok(Self::Ascii85Decode),
            b"RunLengthDecode" | b"RL" => Ok(Self::RunLengthDecode),
            other => Err(Error::Malformed {
                what: match other {
                    b"CCITTFaxDecode" | b"CCF" => return Ok(Filter::CcittFaxDecode),
                    b"DCTDecode" | b"JPXDecode" | b"JBIG2Decode" => {
                        "image filters are out of scope"
                    }
                    b"Crypt" => "the /Crypt filter is Q9's job",
                    _ => "unknown /Filter name",
                },
                offset: None,
            }),
        }
    }

    /// The canonical `/Filter` name.
    pub fn name(self) -> &'static str {
        match self {
            Self::FlateDecode => "FlateDecode",
            Self::LzwDecode => "LZWDecode",
            Self::AsciiHexDecode => "ASCIIHexDecode",
            Self::Ascii85Decode => "ASCII85Decode",
            Self::RunLengthDecode => "RunLengthDecode",
            Self::CcittFaxDecode => "CCITTFaxDecode",
        }
    }
}

/// One entry of the filter chain: the filter plus its `/DecodeParms`.
#[derive(Debug, Clone, PartialEq)]
pub struct FilterSpec {
    /// The filter to apply.
    pub filter: Filter,
    /// The dictionary from `/DecodeParms` for this filter, if any.
    pub parms: Option<Dictionary>,
}

impl FilterSpec {
    /// Read `/Filter` and `/DecodeParms` from a stream dictionary.
    ///
    /// Only direct values are followed; an indirect `/Filter` or
    /// `/DecodeParms` needs [`crate::Document::decode_stream`] to resolve it
    /// first.
    pub fn from_dict(dict: &Dictionary) -> Result<Vec<Self>> {
        let filters = match dict.get(b"Filter") {
            None | Some(Object::Null) => return Ok(Vec::new()),
            Some(Object::Name(name)) => vec![Filter::from_name(name)?],
            Some(Object::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Object::Name(name) => Filter::from_name(name),
                    _ => Err(Error::Malformed {
                        what: "/Filter array entries must be names",
                        offset: None,
                    }),
                })
                .collect::<Result<Vec<_>>>()?,
            Some(Object::Reference(_)) => {
                return Err(Error::Malformed {
                    what: "indirect /Filter needs the document",
                    offset: None,
                })
            }
            Some(_) => {
                return Err(Error::Malformed {
                    what: "/Filter must be a name or an array of names",
                    offset: None,
                })
            }
        };
        let parms = match dict.get(b"DecodeParms") {
            None | Some(Object::Null) => Vec::new(),
            Some(Object::Dictionary(parms)) => vec![Some(parms.clone())],
            Some(Object::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Object::Null => Ok(None),
                    Object::Dictionary(parms) => Ok(Some(parms.clone())),
                    _ => Err(Error::Malformed {
                        what: "/DecodeParms array entries must be dictionaries or null",
                        offset: None,
                    }),
                })
                .collect::<Result<Vec<_>>>()?,
            Some(Object::Reference(_)) => {
                return Err(Error::Malformed {
                    what: "indirect /DecodeParms needs the document",
                    offset: None,
                })
            }
            Some(_) => {
                return Err(Error::Malformed {
                    what: "/DecodeParms must be a dictionary or an array",
                    offset: None,
                })
            }
        };
        Ok(filters
            .into_iter()
            .enumerate()
            .map(|(index, filter)| FilterSpec {
                filter,
                parms: parms.get(index).cloned().flatten(),
            })
            .collect())
    }
}

/// The most bytes one stream may decode to, unless a document sets another
/// budget ([`crate::Document::set_decoded_limit`]).
///
/// 256 MiB, chosen to match mupdf's `FZ_STORE_DEFAULT` rather than invented:
/// real files are orders of magnitude below it, and a decompression bomb is
/// orders of magnitude above. Without a cap a 1 MiB file expands to a gigabyte
/// (measured: 0.43 s into 1 073 741 824 bytes), which in a browser tab is a
/// denial of service rather than a rendering failure.
pub const DEFAULT_DECODE_LIMIT: usize = 256 << 20;

/// Apply a filter chain in order: the first `/Filter` is the outermost.
pub fn decode_chain(data: &[u8], chain: &[FilterSpec]) -> Result<Vec<u8>> {
    decode_chain_with_limit(data, chain, DEFAULT_DECODE_LIMIT)
}

/// Apply a filter chain with an explicit byte budget on its output.
pub fn decode_chain_with_limit(data: &[u8], chain: &[FilterSpec], limit: usize) -> Result<Vec<u8>> {
    if data.len() > limit {
        return Err(Error::Limit {
            what: "the unfiltered stream",
            limit,
        });
    }
    let mut current = data.to_vec();
    for spec in chain {
        current = decode_filter_with_limit(spec.filter, &current, spec.parms.as_ref(), limit)?;
    }
    Ok(current)
}

/// Apply one filter, then its predictor when the filter has one.
pub fn decode_filter(filter: Filter, data: &[u8], parms: Option<&Dictionary>) -> Result<Vec<u8>> {
    decode_filter_with_limit(filter, data, parms, DEFAULT_DECODE_LIMIT)
}

/// Apply one filter under a byte budget, then its predictor under the same one.
///
/// Flate is capped *while* inflating; the other filters
/// expand by a bounded factor and are checked once they return, so the budget
/// still refuses them before the result reaches a caller.
pub fn decode_filter_with_limit(
    filter: Filter,
    data: &[u8],
    parms: Option<&Dictionary>,
    limit: usize,
) -> Result<Vec<u8>> {
    let decoded = match filter {
        Filter::FlateDecode => flate_decode(data, limit)?,
        Filter::LzwDecode => lzw_decode(data, early_change(parms)?)?,
        Filter::AsciiHexDecode => ascii_hex_decode(data)?,
        Filter::Ascii85Decode => ascii85_decode(data)?,
        Filter::RunLengthDecode => run_length_decode(data)?,
        Filter::CcittFaxDecode => return fax_decode(data, parms, limit),
    };
    if decoded.len() > limit {
        return Err(Error::Limit {
            what: filter.name(),
            limit,
        });
    }
    match filter {
        Filter::FlateDecode | Filter::LzwDecode => {
            let predictor = Predictor::from_dict(parms)?;
            let expanded = apply_predictor(&decoded, predictor)?;
            if expanded.len() > limit {
                return Err(Error::Limit {
                    what: filter.name(),
                    limit,
                });
            }
            Ok(expanded)
        }
        _ => Ok(decoded),
    }
}

/// `/CCITTFaxDecode`: the parameters live in `/DecodeParms` (7.4.6).
fn fax_decode(data: &[u8], parms: Option<&Dictionary>, limit: usize) -> Result<Vec<u8>> {
    let mut params = crate::fax::Params::default();
    if let Some(dict) = parms {
        if let Some(k) = dict.get(b"K").and_then(Object::as_integer) {
            params.k = k;
        }
        if let Some(columns) = dict.get(b"Columns").and_then(Object::as_integer) {
            params.columns = u32::try_from(columns)
                .map_err(|_| Error::malformed("fax /Columns must not be negative"))?;
        }
        if let Some(rows) = dict.get(b"Rows").and_then(Object::as_integer) {
            params.rows = u32::try_from(rows)
                .map_err(|_| Error::malformed("fax /Rows must not be negative"))?;
        }
        params.end_of_line = flag(dict, b"EndOfLine");
        params.encoded_byte_align = flag(dict, b"EncodedByteAlign");
        params.end_of_block = match dict.get(b"EndOfBlock") {
            Some(Object::Boolean(value)) => *value,
            _ => true,
        };
        params.black_is_1 = flag(dict, b"BlackIs1");
    }
    crate::fax::decode(data, &params, limit)
}

fn flag(dict: &Dictionary, key: &[u8]) -> bool {
    matches!(dict.get(key), Some(Object::Boolean(true)))
}

impl Stream {
    /// Decode this stream's payload with its own `/Filter` and `/DecodeParms`.
    ///
    /// Fails when either entry is indirect — use
    /// [`crate::Document::decode_stream`] for that case.
    pub fn decode(&self) -> Result<Vec<u8>> {
        let chain = FilterSpec::from_dict(self.dict())?;
        decode_chain(self.data(), &chain)
    }
}

/// `/DecodeParms` predictor settings, with the defaults from §7.4.4.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Predictor {
    /// 1 = none, 2 = TIFF, 10–15 = PNG filters.
    pub predictor: u8,
    /// Colour components per pixel, default 1.
    pub colors: u8,
    /// Bits per component, default 8.
    pub bits_per_component: u8,
    /// Bytes per row divided by the pixel size, default 1.
    pub columns: u16,
}

impl Default for Predictor {
    fn default() -> Self {
        Self {
            predictor: 1,
            colors: 1,
            bits_per_component: 8,
            columns: 1,
        }
    }
}

impl Predictor {
    /// Read predictor settings from a `/DecodeParms` dictionary.
    pub fn from_dict(parms: Option<&Dictionary>) -> Result<Self> {
        let Some(parms) = parms else {
            return Ok(Self::default());
        };
        let mut predictor = Self::default();
        if let Some(Object::Integer(value)) = parms.get(b"Predictor") {
            predictor.predictor = u8::try_from(*value).map_err(|_| Error::Malformed {
                what: "/Predictor out of range",
                offset: None,
            })?;
        }
        if let Some(Object::Integer(value)) = parms.get(b"Colors") {
            predictor.colors = u8::try_from(*value).map_err(|_| Error::Malformed {
                what: "/Colors out of range",
                offset: None,
            })?;
        }
        if let Some(Object::Integer(value)) = parms.get(b"BitsPerComponent") {
            predictor.bits_per_component = u8::try_from(*value).map_err(|_| Error::Malformed {
                what: "/BitsPerComponent out of range",
                offset: None,
            })?;
        }
        if let Some(Object::Integer(value)) = parms.get(b"Columns") {
            predictor.columns = u16::try_from(*value).map_err(|_| Error::Malformed {
                what: "/Columns out of range",
                offset: None,
            })?;
        }
        Ok(predictor)
    }
}

fn flate_decode(data: &[u8], limit: usize) -> Result<Vec<u8>> {
    // PDF Flate streams are zlib-wrapped, but some writers emit raw DEFLATE.
    match inflate_capped(data, DataFormat::Zlib, limit) {
        Ok(out) => return Ok(out),
        // A budget failure is final: retrying the same bytes as raw DEFLATE
        // would double the work and then report the wrong reason.
        Err(Error::Limit { limit, .. }) => {
            return Err(Error::Limit {
                what: "a Flate stream",
                limit,
            })
        }
        Err(_) => {}
    }
    match inflate_capped(data, DataFormat::Raw, limit) {
        Ok(out) => Ok(out),
        Err(Error::Limit { limit, .. }) => Err(Error::Limit {
            what: "a Flate stream",
            limit,
        }),
        Err(_) => Err(Error::Malformed {
            what: "FlateDecode payload",
            offset: None,
        }),
    }
}

/// Inflate with an output budget, stopping as soon as it is passed.
///
/// This is the part that makes a bomb harmless: `decompress_to_vec` grows a
/// vector until the stream ends, so the budget would only be checked after a
/// gigabyte had already been allocated.
fn inflate_capped(data: &[u8], format: DataFormat, limit: usize) -> Result<Vec<u8>> {
    let mut state = InflateState::new(format);
    // Reserve what the input implies (zlib rarely beats 1:4 on real streams)
    // and inflate into a stack buffer: a heap chunk per stream was 110 kB of
    // the benchmark's allocations on a one-page document.
    let mut out: Vec<u8> = Vec::with_capacity(limit.min(data.len().saturating_mul(4).max(64)));
    let mut chunk = [0u8; 32 * 1024];
    let mut consumed = 0usize;
    loop {
        let flush = if consumed >= data.len() {
            MZFlush::Finish
        } else {
            MZFlush::None
        };
        let result = inflate(&mut state, &data[consumed..], &mut chunk, flush);
        consumed += result.bytes_consumed;
        if result.bytes_written > 0 {
            if out.len().saturating_add(result.bytes_written) > limit {
                return Err(Error::Limit {
                    what: "a Flate stream",
                    limit,
                });
            }
            out.extend_from_slice(&chunk[..result.bytes_written]);
        }
        match result.status {
            Ok(MZStatus::StreamEnd) => return Ok(out),
            Ok(MZStatus::Ok) => {
                // No progress with no input left means a truncated stream.
                if consumed >= data.len() && result.bytes_written == 0 {
                    return Err(Error::Malformed {
                        what: "FlateDecode payload",
                        offset: None,
                    });
                }
            }
            _ => {
                return Err(Error::Malformed {
                    what: "FlateDecode payload",
                    offset: None,
                })
            }
        }
    }
}

fn early_change(parms: Option<&Dictionary>) -> Result<bool> {
    match parms.and_then(|parms| parms.get(b"EarlyChange")) {
        None => Ok(true),
        Some(Object::Integer(0)) => Ok(false),
        Some(Object::Integer(1)) => Ok(true),
        Some(_) => Err(Error::Malformed {
            what: "/EarlyChange must be 0 or 1",
            offset: None,
        }),
    }
}

fn ascii_hex_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut high: Option<u8> = None;
    for (offset, &byte) in data.iter().enumerate() {
        if byte == b'>' {
            break;
        }
        if is_whitespace(byte) {
            continue;
        }
        let value = hex_value(byte).ok_or(Error::UnexpectedByte {
            byte,
            offset,
            what: "ASCIIHexDecode payload",
        })?;
        match high.take() {
            Some(high) => out.push((high << 4) | value),
            None => high = Some(value),
        }
    }
    if let Some(high) = high {
        // An odd final digit is the high nibble of a byte padded with 0.
        out.push(high << 4);
    }
    Ok(out)
}

fn ascii85_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut group = [0_u8; 5];
    let mut count = 0;
    let start = if data.starts_with(b"<~") { 2 } else { 0 };
    let mut offset = start;
    while offset < data.len() {
        let byte = data[offset];
        offset += 1;
        if byte == b'~' {
            break;
        }
        if is_whitespace(byte) {
            continue;
        }
        if byte == b'z' {
            if count != 0 {
                return Err(Error::Malformed {
                    what: "ASCII85Decode `z` inside a group",
                    offset: None,
                });
            }
            out.extend_from_slice(&[0, 0, 0, 0]);
            continue;
        }
        if !(b'!'..=b'u').contains(&byte) {
            return Err(Error::UnexpectedByte {
                byte,
                offset: offset - 1,
                what: "ASCII85Decode payload",
            });
        }
        group[count] = byte - b'!';
        count += 1;
        if count == 5 {
            push_ascii85_group(&mut out, &group, 4)?;
            count = 0;
        }
    }
    if count > 0 {
        if count == 1 {
            return Err(Error::Malformed {
                what: "ASCII85Decode final group of one character",
                offset: None,
            });
        }
        for slot in group.iter_mut().skip(count) {
            *slot = 84; // `u`, the largest digit, pads the group
        }
        push_ascii85_group(&mut out, &group, count - 1)?;
    }
    Ok(out)
}

fn push_ascii85_group(out: &mut Vec<u8>, group: &[u8; 5], emit: usize) -> Result<()> {
    let mut value = 0_u64;
    for &digit in group {
        value = value * 85 + u64::from(digit);
    }
    let value = u32::try_from(value).map_err(|_| Error::Malformed {
        what: "ASCII85Decode group overflows 32 bits",
        offset: None,
    })?;
    out.extend_from_slice(&value.to_be_bytes()[..emit]);
    Ok(())
}

fn run_length_decode(data: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < data.len() {
        let length = data[offset];
        offset += 1;
        match length {
            // 128 ends the data.
            128 => break,
            // A length up to 127 is followed by that many plus one literal bytes.
            0..=127 => {
                let count = usize::from(length) + 1;
                let chunk = data
                    .get(offset..offset + count)
                    .ok_or(Error::unexpected_eof("RunLengthDecode literal run"))?;
                out.extend_from_slice(chunk);
                offset += count;
            }
            // 129–255 repeats the next byte 257 minus the length times.
            _ => {
                let byte = *data
                    .get(offset)
                    .ok_or(Error::unexpected_eof("RunLengthDecode repeat run"))?;
                offset += 1;
                out.resize(out.len() + (257 - usize::from(length)), byte);
            }
        }
    }
    Ok(out)
}

const LZW_CLEAR: u16 = 256;
const LZW_EOD: u16 = 257;
const LZW_MAX_CODE: usize = 4096;

fn lzw_decode(data: &[u8], early_change: bool) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut table: Vec<Vec<u8>> = (0..=255_u16).map(|byte| vec![byte as u8]).collect();
    table.push(Vec::new()); // 256: clear table
    table.push(Vec::new()); // 257: end of data
    let mut code_width = 9_u8;
    let mut previous: Option<Vec<u8>> = None;
    let mut bit = 0_usize;
    while let Some(code) = read_lzw_code(data, &mut bit, code_width) {
        if code == LZW_EOD {
            break;
        }
        if code == LZW_CLEAR {
            table.truncate(258);
            code_width = 9;
            previous = None;
            continue;
        }
        let sequence = match table.get(usize::from(code)) {
            Some(entry) if !entry.is_empty() => entry.clone(),
            // The one-code-ahead case: the code names the sequence about to be
            // added, which is the previous sequence plus its own first byte.
            _ if usize::from(code) == table.len() => match &previous {
                Some(previous) => {
                    let mut sequence = previous.clone();
                    sequence.push(previous[0]);
                    sequence
                }
                None => {
                    return Err(Error::Malformed {
                        what: "LZWDecode code without a base sequence",
                        offset: None,
                    })
                }
            },
            _ => {
                return Err(Error::Malformed {
                    what: "LZWDecode code out of range",
                    offset: None,
                })
            }
        };
        out.extend_from_slice(&sequence);
        if let Some(previous) = previous {
            if table.len() < LZW_MAX_CODE {
                let mut entry = previous;
                entry.push(sequence[0]);
                table.push(entry);
                if table.len() + usize::from(early_change) == 512 {
                    code_width = 10;
                } else if table.len() + usize::from(early_change) == 1024 {
                    code_width = 11;
                } else if table.len() + usize::from(early_change) == 2048 {
                    code_width = 12;
                }
            }
        }
        previous = Some(sequence);
    }
    Ok(out)
}

fn read_lzw_code(data: &[u8], bit: &mut usize, width: u8) -> Option<u16> {
    if *bit + usize::from(width) > data.len() * 8 {
        return None;
    }
    let mut value = 0_u16;
    for _ in 0..width {
        let byte = data[*bit / 8];
        let bit_in_byte = 7 - (*bit % 8);
        value = (value << 1) | u16::from((byte >> bit_in_byte) & 1);
        *bit += 1;
    }
    Some(value)
}

fn apply_predictor(data: &[u8], predictor: Predictor) -> Result<Vec<u8>> {
    match predictor.predictor {
        1 => Ok(data.to_vec()),
        2 => tiff_predictor(data, predictor),
        10..=15 => png_predictor(data, predictor),
        _ => Err(Error::Malformed {
            what: "unsupported /Predictor",
            offset: None,
        }),
    }
}

fn png_predictor(data: &[u8], predictor: Predictor) -> Result<Vec<u8>> {
    let colors = usize::from(predictor.colors.max(1));
    let bits = usize::from(predictor.bits_per_component.max(1));
    let columns = usize::from(predictor.columns.max(1));
    let bpp = (colors * bits).div_ceil(8);
    let row_bytes = (columns * colors * bits).div_ceil(8);
    let mut out = Vec::with_capacity(data.len());
    let mut previous = vec![0_u8; row_bytes];
    let mut offset = 0;
    while offset < data.len() {
        let filter = data[offset];
        offset += 1;
        let row = data
            .get(offset..offset + row_bytes)
            .ok_or(Error::unexpected_eof("PNG predictor row"))?;
        offset += row_bytes;
        let mut current = vec![0_u8; row_bytes];
        for index in 0..row_bytes {
            let left = if index >= bpp {
                current[index - bpp]
            } else {
                0
            };
            let up = previous[index];
            let up_left = if index >= bpp {
                previous[index - bpp]
            } else {
                0
            };
            current[index] = match filter {
                0 => row[index],
                1 => row[index].wrapping_add(left),
                2 => row[index].wrapping_add(up),
                3 => row[index].wrapping_add(((u16::from(left) + u16::from(up)) / 2) as u8),
                4 => row[index].wrapping_add(paeth(left, up, up_left)),
                _ => {
                    return Err(Error::Malformed {
                        what: "PNG predictor row filter must be 0–4",
                        offset: None,
                    })
                }
            };
        }
        out.extend_from_slice(&current);
        previous = current;
    }
    Ok(out)
}

fn paeth(left: u8, up: u8, up_left: u8) -> u8 {
    let left = i32::from(left);
    let up = i32::from(up);
    let up_left = i32::from(up_left);
    let prediction = left + up - up_left;
    let distance_left = (prediction - left).abs();
    let distance_up = (prediction - up).abs();
    let distance_up_left = (prediction - up_left).abs();
    if distance_left <= distance_up && distance_left <= distance_up_left {
        left as u8
    } else if distance_up <= distance_up_left {
        up as u8
    } else {
        up_left as u8
    }
}

fn tiff_predictor(data: &[u8], predictor: Predictor) -> Result<Vec<u8>> {
    if predictor.bits_per_component != 8 {
        return Err(Error::Malformed {
            what: "TIFF predictor needs 8 bits per component",
            offset: None,
        });
    }
    let colors = usize::from(predictor.colors.max(1));
    let columns = usize::from(predictor.columns.max(1));
    let row_bytes = colors * columns;
    let mut out = data.to_vec();
    for row in out.chunks_mut(row_bytes) {
        for index in colors..row.len() {
            row[index] = row[index].wrapping_add(row[index - colors]);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{
        apply_predictor, ascii85_decode, ascii_hex_decode, decode_chain, lzw_decode,
        run_length_decode, Filter, FilterSpec, Predictor,
    };
    use crate::error::Error;
    use crate::object::{Dictionary, Object};

    fn parms(entries: &[(&str, Object)]) -> Dictionary {
        let mut dict = Dictionary::new();
        for (name, value) in entries {
            dict.insert(name.as_bytes(), value.clone());
        }
        dict
    }

    #[test]
    fn filter_names_map_to_filters() {
        assert_eq!(
            Filter::from_name(b"FlateDecode").expect("flate"),
            Filter::FlateDecode
        );
        assert_eq!(
            Filter::from_name(b"Fl").expect("flate"),
            Filter::FlateDecode
        );
        assert_eq!(
            Filter::from_name(b"LZWDecode").expect("lzw"),
            Filter::LzwDecode
        );
        assert_eq!(
            Filter::from_name(b"ASCIIHexDecode").expect("ahx"),
            Filter::AsciiHexDecode
        );
        assert_eq!(
            Filter::from_name(b"ASCII85Decode").expect("a85"),
            Filter::Ascii85Decode
        );
        assert_eq!(
            Filter::from_name(b"RunLengthDecode").expect("rl"),
            Filter::RunLengthDecode
        );
        assert_eq!(Filter::FlateDecode.name(), "FlateDecode");
        let image = Filter::from_name(b"DCTDecode").expect_err("image filter");
        assert!(image.to_string().contains("out of scope"), "{image}");
        assert!(Filter::from_name(b"Nope").is_err());
    }

    #[test]
    fn ascii_hex_decodes_with_and_without_the_terminator() {
        assert_eq!(ascii_hex_decode(b"48656C6C6F>").expect("hex"), b"Hello");
        assert_eq!(ascii_hex_decode(b"48 65 6c 6C 6f").expect("hex"), b"Hello");
        assert_eq!(
            ascii_hex_decode(b"414").expect("odd digit"),
            vec![0x41, 0x40]
        );
        assert_eq!(ascii_hex_decode(b">").expect("empty"), Vec::<u8>::new());
        assert!(ascii_hex_decode(b"48G").is_err());
    }

    #[test]
    fn ascii85_decodes_groups_shortcuts_and_partials() {
        // "Hello" as ASCII85 is "87cURDZ".
        assert_eq!(ascii85_decode(b"87cURDZ~>").expect("ascii85"), b"Hello");
        assert_eq!(ascii85_decode(b"87cURDZ").expect("no terminator"), b"Hello");
        assert_eq!(ascii85_decode(b"@:E^").expect("three bytes"), b"abc");
        assert_eq!(ascii85_decode(b"z~>").expect("z shortcut"), [0, 0, 0, 0]);
        assert_eq!(ascii85_decode(b"<~87cURDZ~>").expect("prefix"), b"Hello");
        assert_eq!(ascii85_decode(b"").expect("empty"), Vec::<u8>::new());
        assert!(ascii85_decode(b"87{UR").is_err(), "character out of range");
        assert_eq!(ascii85_decode(b"87").expect("partial").len(), 1);
        assert!(ascii85_decode(b"8").is_err(), "single trailing character");
        assert!(ascii85_decode(b"zZ~>").is_err(), "z inside a group");
    }

    #[test]
    fn run_length_decodes_literal_and_repeat_runs() {
        // 2 literal bytes "AB", then 3 copies of 'x' (257-254=3), then EOD.
        assert_eq!(
            run_length_decode(&[1, b'A', b'B', 254, b'x', 128]).expect("runlength"),
            b"ABxxx"
        );
        assert_eq!(run_length_decode(&[128]).expect("empty"), Vec::<u8>::new());
        assert!(
            run_length_decode(&[5, b'A']).is_err(),
            "truncated literal run"
        );
        assert!(run_length_decode(&[254]).is_err(), "truncated repeat run");
    }

    #[test]
    fn lzw_decodes_a_hand_computed_vector() {
        // 9-bit MSB-first codes 65 ('A'), 66 ('B'), 67 ('C'), 257 (EOD).
        let data = [0x20, 0x90, 0x88, 0x70, 0x10];
        assert_eq!(lzw_decode(&data, true).expect("lzw"), b"ABC");
        assert_eq!(lzw_decode(&data, false).expect("lzw"), b"ABC");
    }

    #[test]
    fn lzw_handles_clear_codes_and_growing_code_widths() {
        // Produced by an independent encoder: clear code, 9-bit codes, width
        // growth at the EarlyChange boundaries, end-of-data. Decoding 360 bytes
        // of text exercises the 9 -> 10 bit step.
        let text: Vec<u8> = b"The quick brown fox jumps over the lazy dog. ".repeat(8);
        let encoded = bytes(concat!(
            "80150d065101c4ea69319ac4062391bcee6e10198de78101a8ea6d381cc406f3",
            "b194e4203a40c406c309e8f220321bcce2e1040a09068442a190e8844a29168c",
            "46a391e9048a4926944aa592e82c1e130b86c3e23138ac5e331b8ec7e43049fc",
            "9e532b96c8a614799d2a6d4d9cd42795391c96ad42accbe8d32a4cd69938a7ce",
            "ea53eb3506b144addb2694b9bd3a75519ed52eb57a1d6ad748bdd7ee17fb1dd2",
            "8184b4d166388af5befd62b9e0b1f68808"
        ));
        assert_eq!(lzw_decode(&encoded, true).expect("lzw"), text);
    }

    fn bytes(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(core::str::from_utf8(pair).expect("hex"), 16).expect("hex")
            })
            .collect()
    }

    #[test]
    fn png_predictors_undo_every_row_filter() {
        // Two rows of three bytes with explicit PNG filter bytes.
        let none = [0, 1, 2, 3, 0, 4, 5, 6];
        let sub = [1, 1, 1, 1, 1, 4, 1, 1];
        let up = [2, 1, 2, 3, 2, 3, 3, 3];
        let average = [3, 1, 2, 2, 3, 4, 2, 2];
        let paeth = [4, 1, 1, 1, 4, 3, 1, 1];
        let settings = Predictor {
            predictor: 15,
            colors: 1,
            bits_per_component: 8,
            columns: 3,
        };
        for encoded in [none, sub, up, average, paeth] {
            assert_eq!(
                apply_predictor(&encoded, settings).expect("predictor"),
                vec![1, 2, 3, 4, 5, 6],
                "encoded {encoded:?}"
            );
        }
        assert!(
            apply_predictor(&[9, 1, 2, 3], settings).is_err(),
            "bad filter byte"
        );
    }

    #[test]
    fn tiff_predictor_is_horizontal_differencing() {
        let encoded = [10, 10, 10, 1, 0, 0];
        let settings = Predictor {
            predictor: 2,
            colors: 1,
            bits_per_component: 8,
            columns: 3,
        };
        assert_eq!(
            apply_predictor(&encoded, settings).expect("tiff"),
            vec![10, 20, 30, 1, 1, 1]
        );
        let rgb = Predictor {
            predictor: 2,
            colors: 3,
            bits_per_component: 8,
            columns: 2,
        };
        assert_eq!(
            apply_predictor(&[1, 2, 3, 10, 10, 10], rgb).expect("tiff rgb"),
            vec![1, 2, 3, 11, 12, 13]
        );
        let unsupported = Predictor {
            predictor: 2,
            bits_per_component: 4,
            ..Predictor::default()
        };
        assert!(apply_predictor(&[0, 0], unsupported).is_err());
        assert!(apply_predictor(
            &[0],
            Predictor {
                predictor: 7,
                ..Predictor::default()
            }
        )
        .is_err());
    }

    #[test]
    fn decoders_report_bad_payloads() {
        assert!(matches!(
            super::decode_filter(Filter::FlateDecode, b"not deflate at all", None),
            Err(Error::Malformed { .. })
        ));
        assert!(matches!(
            super::decode_filter(Filter::Ascii85Decode, b"87{UR", None),
            Err(Error::UnexpectedByte { .. })
        ));
    }

    #[test]
    fn filter_specs_read_names_arrays_and_parms() {
        let dict = parms(&[("Filter", Object::name("FlateDecode"))]);
        let specs = FilterSpec::from_dict(&dict).expect("single filter");
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].filter, Filter::FlateDecode);
        assert!(specs[0].parms.is_none());

        let dict = parms(&[
            (
                "Filter",
                Object::array([Object::name("ASCII85Decode"), Object::name("FlateDecode")]),
            ),
            (
                "DecodeParms",
                Object::array([
                    Object::Null,
                    Object::Dictionary(parms(&[("Predictor", Object::Integer(12))])),
                ]),
            ),
        ]);
        let specs = FilterSpec::from_dict(&dict).expect("filter array");
        assert_eq!(specs.len(), 2);
        assert_eq!(specs[0].filter, Filter::Ascii85Decode);
        assert!(specs[0].parms.is_none());
        assert_eq!(specs[1].filter, Filter::FlateDecode);
        assert!(specs[1].parms.is_some());

        let indirect = parms(&[("Filter", Object::reference(crate::ObjectId::new(9, 0)))]);
        assert!(FilterSpec::from_dict(&indirect).is_err());
        let bad_entry = parms(&[("Filter", Object::array([Object::Integer(1)]))]);
        assert!(FilterSpec::from_dict(&bad_entry).is_err());
    }

    #[test]
    fn chains_apply_in_order() {
        // "Hello" -> ASCII85 -> and back again through a one-filter chain.
        let encoded = b"87cURDZ~>";
        let chain = [FilterSpec {
            filter: Filter::Ascii85Decode,
            parms: None,
        }];
        assert_eq!(decode_chain(encoded, &chain).expect("chain"), b"Hello");
        assert_eq!(decode_chain(b"Hello", &[]).expect("empty chain"), b"Hello");
    }

    #[test]
    fn predictor_defaults_are_inert() {
        assert_eq!(
            Predictor::from_dict(None).expect("defaults"),
            Predictor::default()
        );
        assert_eq!(
            apply_predictor(b"abc", Predictor::default()).expect("none"),
            b"abc"
        );
    }
}

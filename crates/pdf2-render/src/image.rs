//! Image XObjects: decode an 8-bit image, then sample it into the canvas.

use pdf2::{Dictionary, Document, Object, Stream};

use crate::canvas::{BlendMode, Canvas, ClipMask, Color};
use crate::geom::{Matrix, Point};
use crate::{Error, Result};

/// A decoded image: width, height and one colour per pixel, top row first.
#[derive(Debug, Clone)]
pub struct Image {
    width: u32,
    height: u32,
    pixels: Vec<Color>,
    interpolate: bool,
}

impl Image {
    /// Decode an `/Image` XObject.
    ///
    /// Supports 8 bits per component, `DeviceGray`/`DeviceRGB`/`DeviceCMYK`,
    /// `Indexed` images with those bases, and `ICCBased` streams by component
    /// count. Anything else is reported rather than guessed.
    pub fn from_stream(
        document: &Document,
        stream: &Stream,
        id: Option<pdf2::ObjectId>,
    ) -> Result<Self> {
        let dict = stream.dict();
        let width = integer(dict, b"Width")?;
        let height = integer(dict, b"Height")?;
        if width == 0 || height == 0 {
            return Err(Error::malformed("image with zero width or height"));
        }
        let bits = dict
            .get(b"BitsPerComponent")
            .and_then(Object::as_integer)
            .unwrap_or(8);
        // One bit per component is how every fax image arrives; the packed rows
        // are widened to bytes here so the sampler below has one path.
        if bits != 8 && bits != 1 {
            return Err(Error::malformed(
                "only 1 and 8 bits per component images are rendered",
            ));
        }
        let interpolate = matches!(dict.get(b"Interpolate"), Some(Object::Boolean(true)));
        let space = resolve(document, dict.get(b"ColorSpace"))?;
        // The colour space is dictionary metadata, so the size the image claims
        // is known before its stream is touched — and that is the order that
        // keeps a lying /Width from being paid for. Every component is one byte
        // here (8 bits per component is the only depth drawn), so the decoded
        // size is width × height × components.
        let colours = ColourSpace::from_object(document, space)?;
        let per_pixel = colours.components();
        let count = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(per_pixel))
            .ok_or_else(|| Error::malformed("image dimensions overflow"))?;
        let limit = document.decoded_limit();
        if count > limit {
            return Err(Error::Limit {
                what: "an image of the /Width and /Height it declares",
                limit,
            });
        }
        if is_dct(dict) {
            return Self::from_dct(document, stream, id, interpolate);
        }
        let decoded = document.decode_stream(stream)?;
        let data = if bits == 1 {
            widen_one_bit(&decoded, width, height, per_pixel)?
        } else {
            decoded
        };
        if data.len() < count {
            return Err(Error::Malformed {
                what: "image data is shorter than width × height × components",
                offset: None,
            });
        }
        // `/Decode` remaps every sample before the colour space sees it; the
        // default is the identity, and `[1 0]` per component is an inversion
        // (8.9.5.2).
        let decode = decode_ranges(dict.get(b"Decode"), per_pixel);
        let mut pixels = Vec::with_capacity((width as usize) * (height as usize));
        for index in (0..count).step_by(per_pixel) {
            let sample = &data[index..index + per_pixel];
            let mut components = [0.0_f64; 4];
            for (slot, byte) in sample.iter().enumerate().take(per_pixel) {
                let (from, to) = decode[slot];
                components[slot] = (from + f64::from(*byte) / 255.0 * (to - from)).clamp(0.0, 1.0);
            }
            pixels.push(colour_at(&colours, &components));
        }
        Ok(Self {
            width,
            height,
            pixels,
            interpolate,
        })
    }

    /// Decode a `/DCTDecode` image through `zune-jpeg`.
    ///
    /// A JPEG in a PDF has no leading filters to unwind, so the bytes come
    /// straight from [`Document::stream_bytes`] — decrypted, not decoded — and
    /// `zune-jpeg` converts YCbCr to RGB itself. The size is checked against the
    /// budget twice: once from the dictionary, and once from the JPEG's own
    /// header, because the header is the authority on what the decoder produces.
    #[cfg(feature = "dct")]
    fn from_dct(
        document: &Document,
        _stream: &Stream,
        id: Option<pdf2::ObjectId>,
        interpolate: bool,
    ) -> Result<Self> {
        let Some(id) = id else {
            return Err(Error::malformed(
                "a /DCTDecode image without an object number",
            ));
        };
        let data = document.stream_bytes(id)?;
        // PDF 7.4.8: a four-component JPEG from an Adobe writer stores its CMYK
        // samples **inverted** (255 - sample), and its `/Decode` array — when the
        // writer states one — is the same statement in words. Asking the decoder
        // for CMYK rather than RGB is what makes the inversion possible: its RGB
        // conversion assumes the already-uninverted order and comes out black.
        let cmyk = dict_component_count(_stream) == Some(4);
        let mut decoder =
            zune_jpeg::JpegDecoder::new(zune_jpeg::zune_core::bytestream::ZCursor::new(&data));
        if cmyk {
            decoder.set_options(
                zune_jpeg::zune_core::options::DecoderOptions::default()
                    .jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::CMYK),
            );
        }
        let pixels = decoder
            .decode()
            .map_err(|_| Error::malformed("/DCTDecode payload"))?;
        let Some(info) = decoder.info() else {
            return Err(Error::malformed(
                "/DCTDecode payload without a frame header",
            ));
        };
        let components = match decoder.output_colorspace() {
            Some(zune_jpeg::zune_core::colorspace::ColorSpace::Luma) => 1,
            Some(zune_jpeg::zune_core::colorspace::ColorSpace::CMYK) => 4,
            _ => 3,
        };
        let count = usize::from(info.width)
            .checked_mul(usize::from(info.height))
            .and_then(|samples| samples.checked_mul(components))
            .ok_or_else(|| Error::malformed("image dimensions overflow"))?;
        let limit = document.decoded_limit();
        if count > limit {
            return Err(Error::Limit {
                what: "a /DCTDecode image of the size its header declares",
                limit,
            });
        }
        if pixels.len() < count {
            return Err(Error::malformed(
                "/DCTDecode payload is shorter than its header",
            ));
        }
        // The Adobe inversion is applied here only when the writer did not state
        // it as `/Decode`: the two are the same statement, and applying both
        // would invert twice (which is what pdf.js does, and what
        // `cmykjpeg.pdf` — an Adobe APP14 all-zero transform with no `/Decode` —
        // needs).
        let stated_decode = _stream.dict().get(b"Decode").is_some();
        let invert = cmyk && !stated_decode && has_adobe_marker(&data);
        // A `/Decode` array, when the writer states one, is applied to the samples
        // after the Adobe inversion; when it inverts, the two cancel out.
        let decode = decode_ranges(_stream.dict().get(b"Decode"), components);
        let sample = |bytes: &[u8]| -> Vec<f64> {
            bytes
                .iter()
                .enumerate()
                .map(|(index, byte)| {
                    let value = f64::from(*byte) / 255.0;
                    let (from, to) = decode[index.min(decode.len() - 1)];
                    (from + value * (to - from)).clamp(0.0, 1.0)
                })
                .collect()
        };
        let pixels = pixels[..count]
            .chunks_exact(components)
            .map(|bytes| {
                let bytes: Vec<u8> = if invert {
                    bytes.iter().map(|byte| 255 - byte).collect()
                } else {
                    bytes.to_vec()
                };
                let values = sample(&bytes);
                match components {
                    1 => Color::rgb(values[0], values[0], values[0]),
                    4 => Color::from_cmyk(values[0], values[1], values[2], values[3]),
                    _ => Color::rgb(values[0], values[1], values[2]),
                }
            })
            .collect();
        Ok(Self {
            width: u32::from(info.width),
            height: u32::from(info.height),
            pixels,
            interpolate,
        })
    }

    /// Without the feature a JPEG is reported, not guessed at.
    #[cfg(not(feature = "dct"))]
    fn from_dct(
        _document: &Document,
        _stream: &Stream,
        _id: Option<pdf2::ObjectId>,
        _interpolate: bool,
    ) -> Result<Self> {
        Err(Error::Unsupported {
            what: "/DCTDecode images (build with the `dct` feature)",
        })
    }

    /// Width in samples.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in samples.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The colour at a sample position, nearest or bilinear.
    pub fn sample(&self, u: f64, v: f64) -> Color {
        if self.width == 0 || self.height == 0 {
            return Color::WHITE;
        }
        let x = (u.clamp(0.0, 1.0) * f64::from(self.width) - 0.5).max(0.0);
        let y = (v.clamp(0.0, 1.0) * f64::from(self.height) - 0.5).max(0.0);
        if !self.interpolate {
            return self.pixel(x.round() as u32, y.round() as u32);
        }
        let x0 = x.floor() as u32;
        let y0 = y.floor() as u32;
        let x1 = (x0 + 1).min(self.width - 1);
        let y1 = (y0 + 1).min(self.height - 1);
        let fx = x - f64::from(x0);
        let fy = y - f64::from(y0);
        let top = mix(self.pixel(x0, y0), self.pixel(x1, y0), fx);
        let bottom = mix(self.pixel(x0, y1), self.pixel(x1, y1), fx);
        mix(top, bottom, fy)
    }

    fn pixel(&self, x: u32, y: u32) -> Color {
        let x = x.min(self.width - 1);
        let y = y.min(self.height - 1);
        self.pixels[(y as usize) * (self.width as usize) + (x as usize)]
    }

    /// Paint into the canvas through `ctm`, restricted to `mask`.
    pub fn paint(
        &self,
        canvas: &mut Canvas,
        mask: &ClipMask,
        ctm: Matrix,
        alpha: f64,
        mode: BlendMode,
    ) {
        let Some(inverse) = ctm.invert() else {
            return;
        };
        let corners = [
            ctm.apply(Point::new(0.0, 0.0)),
            ctm.apply(Point::new(1.0, 0.0)),
            ctm.apply(Point::new(0.0, 1.0)),
            ctm.apply(Point::new(1.0, 1.0)),
        ];
        let min_x = corners
            .iter()
            .fold(f64::INFINITY, |min, point| min.min(point.x))
            .floor()
            .max(0.0);
        let max_x = corners
            .iter()
            .fold(f64::NEG_INFINITY, |max, point| max.max(point.x))
            .ceil()
            .min(f64::from(canvas.width()));
        let min_y = corners
            .iter()
            .fold(f64::INFINITY, |min, point| min.min(point.y))
            .floor()
            .max(0.0);
        let max_y = corners
            .iter()
            .fold(f64::NEG_INFINITY, |max, point| max.max(point.y))
            .ceil()
            .min(f64::from(canvas.height()));
        for y in min_y as u32..max_y as u32 {
            for x in min_x as u32..max_x as u32 {
                let coverage = mask.alpha(x, y);
                if coverage <= 0.0 {
                    continue;
                }
                let device = Point::new(f64::from(x) + 0.5, f64::from(y) + 0.5);
                let user = inverse.apply(device);
                if !(0.0..=1.0).contains(&user.x) || !(0.0..=1.0).contains(&user.y) {
                    continue;
                }
                // Image space has its origin at the bottom-left, but the first
                // sample row is the top one.
                let colour = self.sample(user.x, 1.0 - user.y);
                canvas.blend_with(x, y, colour, alpha * coverage, mode);
            }
        }
    }
}

/// The `/Decode` range for each component, defaulting to the identity.
fn decode_ranges(decode: Option<&Object>, components: usize) -> Vec<(f64, f64)> {
    let mut ranges = vec![(0.0, 1.0); components];
    let Some(Object::Array(items)) = decode else {
        return ranges;
    };
    if items.len() < components * 2 {
        return ranges;
    }
    for (slot, pair) in items.chunks_exact(2).enumerate().take(components) {
        let (Some(from), Some(to)) = (pair[0].as_real(), pair[1].as_real()) else {
            continue;
        };
        ranges[slot] = (from, to);
    }
    ranges
}

/// Turn decoded components (0–1 each) into a colour through `colours`.
fn colour_at(colours: &ColourSpace, components: &[f64; 4]) -> Color {
    match colours {
        ColourSpace::Gray => Color::rgb(components[0], components[0], components[0]),
        ColourSpace::Rgb => Color::rgb(components[0], components[1], components[2]),
        ColourSpace::Cmyk => {
            Color::from_cmyk(components[0], components[1], components[2], components[3])
        }
        ColourSpace::Indexed { base, lookup } => {
            // For an indexed image the decoded value is the index, not a colour
            // component, so it is scaled back into the table's range.
            let index = (components[0] * 255.0).round().max(0.0) as usize;
            let count = base.components();
            let entry = index * count;
            let slice = lookup.get(entry..entry + count).unwrap_or(&[]);
            if slice.len() < count {
                return Color::BLACK;
            }
            let mut table = [0.0_f64; 4];
            for (slot, byte) in slice.iter().enumerate().take(4) {
                table[slot] = f64::from(*byte) / 255.0;
            }
            colour_at(base, &table)
        }
    }
}

fn mix(left: Color, right: Color, t: f64) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color::rgb(
        left.r * (1.0 - t) + right.r * t,
        left.g * (1.0 - t) + right.g * t,
        left.b * (1.0 - t) + right.b * t,
    )
}

/// The colour space of an image, resolved to what the sampler needs.
#[derive(Debug, Clone, PartialEq)]
enum ColourSpace {
    Gray,
    Rgb,
    Cmyk,
    Indexed {
        base: Box<ColourSpace>,
        lookup: Vec<u8>,
    },
}

impl ColourSpace {
    fn from_object(document: &Document, object: Object) -> Result<Self> {
        match object {
            Object::Name(name) => match name.as_slice() {
                b"DeviceGray" | b"G" | b"CalGray" => Ok(Self::Gray),
                b"DeviceRGB" | b"RGB" | b"CalRGB" => Ok(Self::Rgb),
                b"DeviceCMYK" | b"CMYK" => Ok(Self::Cmyk),
                _ => Err(Error::malformed("unsupported image /ColorSpace")),
            },
            Object::Array(items) => {
                let family = match items.first() {
                    Some(Object::Name(name)) => name.clone(),
                    _ => return Err(Error::malformed("image /ColorSpace array family")),
                };
                match family.as_slice() {
                    b"Indexed" | b"I" => {
                        let base = match items.get(1) {
                            Some(base) => {
                                Self::from_object(document, resolve(document, Some(base))?)?
                            }
                            None => return Err(Error::malformed("Indexed image without a base")),
                        };
                        let lookup = match items.get(3) {
                            Some(Object::String(string)) => string.as_bytes().to_vec(),
                            Some(Object::Reference(id)) => document.stream_data(*id)?,
                            _ => {
                                return Err(Error::malformed(
                                    "Indexed image without a lookup table",
                                ))
                            }
                        };
                        Ok(Self::Indexed {
                            base: Box::new(base),
                            lookup,
                        })
                    }
                    b"ICCBased" => {
                        let stream = match items.get(1) {
                            Some(Object::Reference(id)) => document.stream(*id)?,
                            _ => return Err(Error::malformed("ICCBased without a stream")),
                        };
                        let components = stream
                            .dict()
                            .get(b"N")
                            .and_then(Object::as_integer)
                            .unwrap_or(3);
                        match components {
                            1 => Ok(Self::Gray),
                            3 => Ok(Self::Rgb),
                            4 => Ok(Self::Cmyk),
                            _ => Err(Error::malformed("unsupported ICCBased components")),
                        }
                    }
                    _ => Err(Error::malformed("unsupported image /ColorSpace array")),
                }
            }
            _ => Err(Error::malformed("unsupported image /ColorSpace value")),
        }
    }

    fn components(&self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
            Self::Cmyk => 4,
            Self::Indexed { .. } => 1,
        }
    }
}

fn integer(dict: &Dictionary, key: &[u8]) -> Result<u32> {
    dict.get(key)
        .and_then(Object::as_integer)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| Error::malformed("image without integer /Width and /Height"))
}

/// How many components the stream's `/ColorSpace` declares, when it is a name or
/// a known family — enough to know whether a JPEG is CMYK.
fn dict_component_count(stream: &Stream) -> Option<usize> {
    match stream.dict().get(b"ColorSpace") {
        Some(Object::Name(name)) if name == b"DeviceCMYK" => Some(4),
        Some(Object::Name(name)) if name == b"DeviceGray" => Some(1),
        Some(Object::Name(name)) if name == b"DeviceRGB" => Some(3),
        _ => None,
    }
}

/// Widen packed one-bit samples to one byte each, 0 or 255.
///
/// PDF pads each row to a byte boundary, so a 1728-pixel fax line occupies 216
/// bytes; a row shorter than its padding is malformed rather than merely short.
fn widen_one_bit(data: &[u8], width: u32, height: u32, components: usize) -> Result<Vec<u8>> {
    if components != 1 {
        return Err(Error::malformed(
            "one-bit samples need a single-component colour space",
        ));
    }
    let bits_per_row = usize::try_from(width).unwrap_or(0);
    let stride = bits_per_row.div_ceil(8);
    let rows = usize::try_from(height).unwrap_or(0);
    if data.len() < stride.saturating_mul(rows) {
        return Err(Error::malformed(
            "one-bit image data is shorter than its rows",
        ));
    }
    let mut out = Vec::with_capacity(bits_per_row.saturating_mul(rows));
    for row in 0..rows {
        for column in 0..bits_per_row {
            let byte = data[row * stride + column / 8];
            let shift = 7 - column % 8;
            let bit = (byte >> shift) & 1;
            out.push(if bit == 1 { 255 } else { 0 });
        }
    }
    Ok(out)
}

/// Whether a JPEG carries an Adobe APP14 marker, which is what makes its CMYK
/// samples stored inverted (PDF 7.4.8).
///
/// The marker is `FFEE` then a two-byte length and the identifier `Adobe`; only
/// the identifier matters here, because the transform byte says whether the
/// samples are plain CMYK or YCCK, and both are stored inverted.
fn has_adobe_marker(data: &[u8]) -> bool {
    let mut index = 2; // past the `FF D8` start-of-image marker
    while index + 4 <= data.len() {
        if data[index] != 0xFF {
            return false;
        }
        let marker = data[index + 1];
        // Segments with no payload.
        if matches!(marker, 0xD8 | 0x01 | 0xD0..=0xD7) {
            index += 2;
            continue;
        }
        let length = usize::from(u16::from_be_bytes([data[index + 2], data[index + 3]]));
        if length < 2 {
            return false;
        }
        if marker == 0xEE {
            let payload = &data[index + 4..(index + 2 + length).min(data.len())];
            return payload.starts_with(b"Adobe");
        }
        // Start of scan: no more header segments follow.
        if marker == 0xDA {
            return false;
        }
        index += 2 + length;
    }
    false
}

/// Whether the stream's filter chain ends in `/DCTDecode`.
fn is_dct(dict: &Dictionary) -> bool {
    match dict.get(b"Filter") {
        Some(Object::Name(name)) => name == b"DCTDecode",
        Some(Object::Array(items)) => items
            .iter()
            .rev()
            .find_map(Object::as_name)
            .is_some_and(|name| name == b"DCTDecode"),
        _ => false,
    }
}

/// Follow one indirect reference; deeper chains are not needed for images.
pub(crate) fn resolve(document: &Document, object: Option<&Object>) -> Result<Object> {
    match object {
        Some(Object::Reference(id)) => document.get_object(*id),
        Some(other) => Ok(other.clone()),
        None => Ok(Object::Null),
    }
}

#[cfg(test)]
mod tests {
    use super::Image;
    use crate::canvas::{BlendMode, Canvas, ClipMask, Color};
    use crate::geom::Matrix;

    fn image(width: u32, height: u32, interpolate: bool) -> Image {
        let mut pixels = Vec::new();
        for y in 0..height {
            for x in 0..width {
                pixels.push(if (x + y) % 2 == 0 {
                    Color::BLACK
                } else {
                    Color::WHITE
                });
            }
        }
        Image {
            width,
            height,
            pixels,
            interpolate,
        }
    }

    #[test]
    fn nearest_sampling_returns_whole_pixels() {
        let image = image(2, 2, false);
        assert_eq!(image.sample(0.25, 0.25), Color::BLACK);
        assert_eq!(image.sample(0.75, 0.25), Color::WHITE);
        assert_eq!(image.sample(0.25, 0.75), Color::WHITE);
    }

    #[test]
    fn bilinear_sampling_blends_neighbours() {
        let image = image(2, 2, true);
        let middle = image.sample(0.5, 0.5);
        assert!((middle.r - 0.5).abs() < 1e-9, "{middle:?}");
    }

    #[test]
    fn painting_covers_the_unit_square_transform() {
        let image = image(2, 2, false);
        let mut canvas = Canvas::new(4, 4, Color::WHITE);
        let mask = ClipMask::all(4, 4);
        // Unit square scaled to 4×4 with y flipped like the page transform.
        let ctm = Matrix::new(4.0, 0.0, 0.0, -4.0, 0.0, 4.0);
        image.paint(&mut canvas, &mask, ctm, 1.0, BlendMode::Normal);
        assert_eq!(canvas.pixel(0, 0), Some([0, 0, 0, 255]));
        assert_eq!(canvas.pixel(3, 0), Some([255, 255, 255, 255]));
        assert_eq!(canvas.pixel(0, 3), Some([255, 255, 255, 255]));
        assert_eq!(canvas.pixel(3, 3), Some([0, 0, 0, 255]));
    }
}

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
    pub fn from_stream(document: &Document, stream: &Stream) -> Result<Self> {
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
        if bits != 8 {
            return Err(Error::malformed(
                "only 8 bits per component images are rendered",
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
        let data = document.decode_stream(stream)?;
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

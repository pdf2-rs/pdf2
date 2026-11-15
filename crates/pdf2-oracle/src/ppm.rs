//! A minimal PPM (P6) reader and the comparison metrics.
//!
//! PPM is what the oracles can all produce without a decoder dependency:
//! `pdftoppm` writes it by default, `mutool draw` writes it for a `.ppm` output
//! name, and Ghostscript's `ppmraw` device writes nothing else.

/// An 8-bit RGB image.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ppm {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 3` bytes, row-major.
    pub pixels: Vec<u8>,
}

impl Ppm {
    /// Parse a binary PPM (`P6`) with a maximum value of 255.
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut fields = Vec::new();
        let mut index = 0;
        // Header: P6, width, height, maxval, each separated by whitespace, with
        // `#` comments allowed anywhere the whitespace is.
        while fields.len() < 4 {
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if index < bytes.len() && bytes[index] == b'#' {
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                continue;
            }
            let start = index;
            while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                index += 1;
            }
            if start == index {
                return Err("truncated PPM header".to_owned());
            }
            fields.push(&bytes[start..index]);
        }
        if fields[0] != b"P6" {
            return Err(format!(
                "not a binary PPM: {:?}",
                String::from_utf8_lossy(fields[0])
            ));
        }
        let number = |field: &[u8], what: &str| -> Result<u32, String> {
            core::str::from_utf8(field)
                .ok()
                .and_then(|text| text.parse().ok())
                .ok_or_else(|| format!("PPM {what} is not a number"))
        };
        let width = number(fields[1], "width")?;
        let height = number(fields[2], "height")?;
        let maxval = number(fields[3], "maxval")?;
        if maxval != 255 {
            return Err(format!("PPM maxval {maxval} is not 8-bit"));
        }
        // Exactly one whitespace byte separates the header from the raster.
        index += 1;
        let expected = (width as usize) * (height as usize) * 3;
        let pixels = bytes
            .get(index..index + expected)
            .ok_or_else(|| format!("PPM raster is short: expected {expected} bytes"))?;
        Ok(Self {
            width,
            height,
            pixels: pixels.to_vec(),
        })
    }
}

/// How far two images are apart, at two scales.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distance {
    /// Mean absolute channel difference, 0–255.
    pub mean: f64,
    /// Fraction of pixels with any channel differing by more than 32/255.
    pub over: f64,
}

/// Downsample by averaging `blocks x blocks` pixel blocks, the scale at which a
/// renderer without anti-aliasing can be compared with one that has it.
pub fn downsample(image: &Ppm, blocks: u32) -> Ppm {
    let width = (image.width / blocks).max(1);
    let height = (image.height / blocks).max(1);
    let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 3);
    let count = f64::from(blocks * blocks);
    for y in 0..height {
        for x in 0..width {
            for channel in 0..3 {
                let mut sum = 0.0;
                for by in 0..blocks {
                    for bx in 0..blocks {
                        let px = (x * blocks + bx).min(image.width - 1);
                        let py = (y * blocks + by).min(image.height - 1);
                        let at =
                            ((py as usize) * (image.width as usize) + px as usize) * 3 + channel;
                        sum += f64::from(image.pixels[at]);
                    }
                }
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let value = (sum / count).round().clamp(0.0, 255.0) as u8;
                pixels.push(value);
            }
        }
    }
    Ppm {
        width,
        height,
        pixels,
    }
}

/// Compare two images of the same size.
pub fn distance(left: &Ppm, right: &Ppm) -> Result<Distance, String> {
    if left.width != right.width || left.height != right.height {
        return Err(format!(
            "size mismatch: {}x{} against {}x{}",
            left.width, left.height, right.width, right.height
        ));
    }
    let mut total = 0_u64;
    let mut over = 0_u64;
    for (a, b) in left.pixels.iter().zip(&right.pixels) {
        total += u64::from(a.abs_diff(*b));
    }
    let (left_pixels, _) = left.pixels.as_chunks::<3>();
    let (right_pixels, _) = right.pixels.as_chunks::<3>();
    for (pa, pb) in left_pixels.iter().zip(right_pixels) {
        if pa.iter().zip(pb).any(|(a, b)| a.abs_diff(*b) > 32) {
            over += 1;
        }
    }
    let channels = (left.pixels.len()).max(1) as f64;
    let pixels = (left.width as u64 * left.height as u64).max(1) as f64;
    Ok(Distance {
        mean: total as f64 / channels,
        over: over as f64 / pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::{distance, downsample, Ppm};

    fn solid(width: u32, height: u32, colour: [u8; 3]) -> Ppm {
        let mut pixels = Vec::new();
        for _ in 0..width * height {
            pixels.extend_from_slice(&colour);
        }
        Ppm {
            width,
            height,
            pixels,
        }
    }

    #[test]
    fn a_ppm_with_comments_parses() {
        let mut bytes = b"P6\n# made by a test\n2 1\n255\n".to_vec();
        bytes.extend_from_slice(&[255, 0, 0, 0, 255, 0]);
        let ppm = Ppm::parse(&bytes).expect("parse");
        assert_eq!((ppm.width, ppm.height), (2, 1));
        assert_eq!(ppm.pixels, vec![255, 0, 0, 0, 255, 0]);
    }

    #[test]
    fn a_short_raster_is_an_error_not_a_panic() {
        let bytes = b"P6\n2 1\n255\n\xff\x00\x00";
        assert!(Ppm::parse(bytes).is_err());
    }

    #[test]
    fn identical_images_are_zero_apart() {
        let image = solid(4, 4, [10, 20, 30]);
        let distance = distance(&image, &image).expect("distance");
        assert_eq!(distance.mean, 0.0);
        assert_eq!(distance.over, 0.0);
    }

    #[test]
    fn a_difference_is_measured_per_channel_and_per_pixel() {
        let left = solid(2, 1, [0, 0, 0]);
        let mut right = left.clone();
        right.pixels[3] = 40; // second pixel's red channel
        let distance = distance(&left, &right).expect("distance");
        assert!((distance.mean - 40.0 / 6.0).abs() < 1e-9, "{distance:?}");
        assert!((distance.over - 0.5).abs() < 1e-9, "{distance:?}");
    }

    #[test]
    fn downsampling_averages_blocks() {
        let image = solid(4, 4, [100, 100, 100]);
        let small = downsample(&image, 2);
        assert_eq!((small.width, small.height), (2, 2));
        assert!(small.pixels.iter().all(|value| *value == 100));
    }
}

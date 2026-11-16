//! The RGBA8 canvas and the clip mask that goes with it.

/// A colour with straight (non-premultiplied) components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Red, 0–1.
    pub r: f64,
    /// Green, 0–1.
    pub g: f64,
    /// Blue, 0–1.
    pub b: f64,
}

impl Color {
    /// Black.
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    /// White.
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    /// A colour from three components in 0–1.
    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }

    /// A colour from three bytes.
    pub fn from_bytes(r: u8, g: u8, b: u8) -> Self {
        Self::rgb(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
        )
    }

    /// The colour as three bytes, clamped.
    pub fn to_bytes(self) -> [u8; 3] {
        [
            component_to_byte(self.r),
            component_to_byte(self.g),
            component_to_byte(self.b),
        ]
    }

    /// CMYK to RGB, the naive conversion PDF's `/DeviceCMYK` implies without a
    /// colour space.
    pub fn from_cmyk(c: f64, m: f64, y: f64, k: f64) -> Self {
        let c = c.clamp(0.0, 1.0);
        let m = m.clamp(0.0, 1.0);
        let y = y.clamp(0.0, 1.0);
        let k = k.clamp(0.0, 1.0);
        Self::rgb(
            (1.0 - c) * (1.0 - k),
            (1.0 - m) * (1.0 - k),
            (1.0 - y) * (1.0 - k),
        )
    }
}

fn component_to_byte(value: f64) -> u8 {
    let scaled = (value.clamp(0.0, 1.0) * 255.0).round();
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = scaled as u8;
    byte
}

/// An RGBA8 raster.
#[derive(Debug, Clone, PartialEq)]
pub struct Canvas {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    /// The pixel value a blank canvas holds, as RGBA bytes.
    background: [u8; 4],
}

impl Canvas {
    /// A canvas filled with `background`.
    pub fn new(width: u32, height: u32, background: Color) -> Self {
        let [r, g, b] = background.to_bytes();
        let background = [r, g, b, 255];
        let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for _ in 0..(width as usize) * (height as usize) {
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
        Self {
            width,
            height,
            pixels,
            background,
        }
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The raw RGBA8 pixels, row-major from the top.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// The RGBA value at `(x, y)`, or `None` outside the canvas.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let slice = &self.pixels[index..index + 4];
        Some([slice[0], slice[1], slice[2], slice[3]])
    }

    /// Whether every pixel is the background colour.
    ///
    /// Used to tell "this page is empty" from "this page is drawn": a page that
    /// ran painting operators and left the canvas uniform is reported as
    /// `drew-nothing` rather than as fully supported.
    pub fn is_blank(&self) -> bool {
        let background = self.background;
        self.pixels.chunks_exact(4).all(|pixel| pixel == background)
    }

    /// Paint one pixel with `color` at `alpha`, source-over.
    pub fn blend(&mut self, x: u32, y: u32, color: Color, alpha: f64) {
        if x >= self.width || y >= self.height || alpha <= 0.0 {
            return;
        }
        let alpha = alpha.clamp(0.0, 1.0);
        let [r, g, b] = color.to_bytes();
        let index = ((y as usize) * (self.width as usize) + (x as usize)) * 4;
        let target = &mut self.pixels[index..index + 4];
        let mix = |source: u8, destination: u8| -> u8 {
            let source = f64::from(source);
            let destination = f64::from(destination);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let value = (source * alpha + destination * (1.0 - alpha)).round() as u8;
            value
        };
        target[0] = mix(r, target[0]);
        target[1] = mix(g, target[1]);
        target[2] = mix(b, target[2]);
        target[3] = 255;
    }
}

/// A per-pixel inclusion mask produced by clipping.
///
/// `None` means "no clip": every pixel is inside. A `Some(mask)` is row-major,
/// one bool per pixel, and painting is restricted to the set entries.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipMask {
    width: u32,
    height: u32,
    /// Coverage per pixel, 0 (outside) to 255 (fully inside). A soft mask is the
    /// same structure with values in between: that is what makes a luminosity
    /// `/SMask` a multiply here rather than a second mechanism everywhere else.
    coverage: Vec<u8>,
}

impl ClipMask {
    /// A mask that contains every pixel of a `width × height` canvas.
    pub fn all(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            coverage: vec![255; (width as usize) * (height as usize)],
        }
    }

    /// An empty mask: no pixel is inside.
    pub fn none(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            coverage: vec![0; (width as usize) * (height as usize)],
        }
    }

    /// Whether `(x, y)` is inside the mask.
    pub fn contains(&self, x: u32, y: u32) -> bool {
        if x >= self.width || y >= self.height {
            return false;
        }
        self.coverage[(y as usize) * (self.width as usize) + (x as usize)] > 0
    }

    /// How much of `(x, y)` is inside, 0.0 to 1.0.
    pub fn alpha(&self, x: u32, y: u32) -> f64 {
        if x >= self.width || y >= self.height {
            return 0.0;
        }
        f64::from(self.coverage[(y as usize) * (self.width as usize) + (x as usize)]) / 255.0
    }

    /// Multiply this mask's coverage by a rendered group's luminosity (11.6.5.2).
    ///
    /// `group` is the soft mask's `/G` rendered onto black, so an unpainted area
    /// has luminance 0 and hides everything, and white shows everything.
    pub fn multiply_luminosity(&mut self, group: &Canvas) {
        if group.width != self.width || group.height != self.height {
            return;
        }
        for y in 0..self.height {
            for x in 0..self.width {
                let pixel = group.pixel(x, y).unwrap_or([0, 0, 0, 255]);
                let luminance = 0.3 * f64::from(pixel[0])
                    + 0.59 * f64::from(pixel[1])
                    + 0.11 * f64::from(pixel[2]);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let coverage = (luminance * 255.0 / 255.0).round() as u8;
                let index = (y as usize) * (self.width as usize) + (x as usize);
                let scaled = u16::from(self.coverage[index]) * u16::from(coverage) / 255;
                #[allow(clippy::cast_possible_truncation)]
                let scaled = scaled as u8;
                self.coverage[index] = scaled;
            }
        }
    }

    /// Intersect this mask with a new coverage mask of the same size.
    pub fn intersect(&mut self, other: &Self) {
        if other.width != self.width || other.height != self.height {
            return;
        }
        for (coverage, other) in self.coverage.iter_mut().zip(&other.coverage) {
            let product = u16::from(*coverage) * u16::from(*other) / 255;
            *coverage = u8::try_from(product).unwrap_or(u8::MAX);
        }
    }

    /// Set one pixel.
    pub fn set(&mut self, x: u32, y: u32, inside: bool) {
        if x < self.width && y < self.height {
            let value = if inside { 255 } else { 0 };
            self.coverage[(y as usize) * (self.width as usize) + (x as usize)] = value;
        }
    }

    /// How many pixels are inside, for tests.
    pub fn count(&self) -> usize {
        self.coverage
            .iter()
            .filter(|coverage| **coverage > 0)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::{Canvas, ClipMask, Color};

    #[test]
    fn a_new_canvas_is_uniformly_filled() {
        let canvas = Canvas::new(3, 2, Color::WHITE);
        assert_eq!(canvas.width(), 3);
        assert_eq!(canvas.height(), 2);
        assert_eq!(canvas.pixels().len(), 3 * 2 * 4);
        for y in 0..2 {
            for x in 0..3 {
                assert_eq!(canvas.pixel(x, y), Some([255, 255, 255, 255]));
            }
        }
        assert_eq!(canvas.pixel(3, 0), None);
    }

    #[test]
    fn blending_mixes_towards_the_source() {
        let mut canvas = Canvas::new(1, 1, Color::WHITE);
        canvas.blend(0, 0, Color::BLACK, 1.0);
        assert_eq!(canvas.pixel(0, 0), Some([0, 0, 0, 255]));
        let mut canvas = Canvas::new(1, 1, Color::BLACK);
        canvas.blend(0, 0, Color::WHITE, 0.5);
        assert_eq!(canvas.pixel(0, 0), Some([128, 128, 128, 255]));
        canvas.blend(0, 0, Color::WHITE, 0.0);
        assert_eq!(canvas.pixel(0, 0), Some([128, 128, 128, 255]));
    }

    #[test]
    fn cmyk_conversion_is_the_naive_one() {
        let red = Color::from_cmyk(0.0, 1.0, 1.0, 0.0);
        assert_eq!(red.to_bytes(), [255, 0, 0]);
        assert_eq!(Color::from_cmyk(0.0, 0.0, 0.0, 1.0).to_bytes(), [0, 0, 0]);
    }

    #[test]
    fn clip_masks_intersect() {
        let mut mask = ClipMask::all(4, 2);
        assert_eq!(mask.count(), 8);
        mask.set(1, 1, false);
        assert!(!mask.contains(1, 1));
        let mut other = ClipMask::all(4, 2);
        other.set(0, 0, false);
        mask.intersect(&other);
        assert!(!mask.contains(0, 0));
        assert!(mask.contains(2, 0));
        assert_eq!(mask.count(), 6);
        assert!(!mask.contains(4, 0));
    }
}

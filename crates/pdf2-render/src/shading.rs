//! Shadings (`8.7.4`): axial, radial and function-based gradients.
//!
//! A shading maps a *parameter* `t` — how far along an axis, how far out from a
//! centre, or where a point sits in a box — through a `/Function` to a colour, and
//! paints it over a region. Types 1 (function-based), 2 (axial) and 3 (radial) are
//! read here; the mesh types (4–7) have no corpus page and are reported by name.
//!
//! Painting is per pixel inside the batch's bounding box and the current clip,
//! which is slower than a scanline gradient but is the same code for all three
//! types and cannot drift from what the `sh` operator asked for.

use pdf2::{Dictionary, Document, Object};

use crate::canvas::{BlendMode, Canvas, ClipMask, Color};
use crate::function::Function;
use crate::geom::Matrix;
use crate::{Error, Result};

/// A parsed shading.
#[derive(Debug, Clone)]
pub(crate) struct Shading {
    space: Space,
    /// `/Matrix` maps shading space into the default space (type 1).
    matrix: Matrix,
    kind: Kind,
}

#[derive(Debug, Clone)]
pub(crate) enum Space {
    Gray,
    Rgb,
    Cmyk,
    /// `/Separation` (8.6.6.4): one tint value through its function into the
    /// alternate space.
    Separation {
        alternate: Box<Space>,
        tint: Function,
    },
    /// `/DeviceN` (8.6.6.5): N tint values through one function.
    DeviceN {
        alternate: Box<Space>,
        tint: Function,
    },
}

#[derive(Debug, Clone)]
enum Kind {
    /// Type 1: `f(x, y)` over `/Domain`, clipped to `/BBox`.
    Function {
        domain: [f64; 4],
        bbox: [f64; 4],
        function: Function,
    },
    /// Type 2: `f(t)` along a line, `t` the projection of the point onto it.
    Axial {
        coords: [f64; 4],
        function: Function,
        extend: [bool; 2],
    },
    /// Type 3: `f(t)` outwards from two circles.
    Radial {
        coords: [f64; 6],
        function: Function,
        extend: [bool; 2],
    },
}

impl Shading {
    /// Read a shading from an object that may be a reference or a dictionary.
    pub(crate) fn from_object(document: &Document, object: &Object) -> Result<Self> {
        let object = match object {
            Object::Reference(id) => document.get_object(*id)?,
            other => other.clone(),
        };
        let Object::Dictionary(dict) = object else {
            return Err(Error::malformed("a /Shading that is not a dictionary"));
        };
        Self::parse(document, &dict)
    }

    /// The colours this shading produces, as a fill would use them: `sh` paints
    /// it whole, a shading pattern paints it where a path fills.
    pub(crate) fn paint_through(
        &self,
        canvas: &mut Canvas,
        mask: &ClipMask,
        alpha: f64,
        mode: BlendMode,
    ) {
        paint(self, canvas, mask, alpha, mode);
    }

    /// Read a shading dictionary.
    pub(crate) fn parse(document: &Document, dict: &Dictionary) -> Result<Self> {
        let space_object = match dict.get(b"ColorSpace") {
            Some(Object::Reference(id)) => document.get_object(*id)?,
            Some(other) => other.clone(),
            None => return Err(Error::malformed("a shading without a /ColorSpace")),
        };
        let space = parse_space(document, &space_object)?;
        let function = parse_function(document, dict.get(b"Function"))?;
        let matrix = dict
            .get(b"Matrix")
            .and_then(Object::as_array)
            .and_then(matrix_operand)
            .unwrap_or(Matrix::IDENTITY);
        let kind = match dict.get(b"ShadingType").and_then(Object::as_integer) {
            Some(1) => Kind::Function {
                domain: numbers(dict, b"Domain", 4).unwrap_or([0.0, 1.0, 0.0, 1.0]),
                bbox: numbers(dict, b"BBox", 4).unwrap_or([0.0, 0.0, 1.0, 1.0]),
                function,
            },
            Some(2) => Kind::Axial {
                coords: numbers(dict, b"Coords", 4)
                    .ok_or(Error::malformed("an axial shading without /Coords"))?,
                function,
                extend: flags(dict, b"Extend"),
            },
            Some(3) => Kind::Radial {
                coords: numbers(dict, b"Coords", 6)
                    .ok_or(Error::malformed("a radial shading without /Coords"))?,
                function,
                extend: flags(dict, b"Extend"),
            },
            Some(4..=7) => {
                return Err(Error::Unsupported {
                    what: "mesh shadings (types 4 to 7)",
                });
            }
            _ => return Err(Error::malformed("a shading without a known /ShadingType")),
        };
        Ok(Self {
            space,
            matrix,
            kind,
        })
    }

    /// The colour of the shading at `(x, y)` in the *shading's* space, if the
    /// point is inside its region.
    pub(crate) fn colour_at(&self, x: f64, y: f64) -> Option<Color> {
        let t = match &self.kind {
            Kind::Function {
                domain,
                bbox,
                function,
            } => {
                let sx = if bbox[2] > bbox[0] {
                    (x - bbox[0]) / (bbox[2] - bbox[0])
                } else {
                    0.0
                };
                let sy = if bbox[3] > bbox[1] {
                    (y - bbox[1]) / (bbox[3] - bbox[1])
                } else {
                    0.0
                };
                let px = domain[0] + sx * (domain[1] - domain[0]);
                let py = domain[2] + sy * (domain[3] - domain[2]);
                let values = function.eval(&[px, py]);
                return Some(self.color(&values));
            }
            Kind::Axial { coords, extend, .. } => {
                let (x0, y0, x1, y1) = (coords[0], coords[1], coords[2], coords[3]);
                let (dx, dy) = (x1 - x0, y1 - y0);
                let length = dx * dx + dy * dy;
                if length <= 0.0 {
                    return None;
                }
                let t = ((x - x0) * dx + (y - y0) * dy) / length;
                if (t < 0.0 && !extend[0]) || (t > 1.0 && !extend[1]) {
                    return None;
                }
                t.clamp(0.0, 1.0)
            }
            Kind::Radial { coords, extend, .. } => {
                // The parameter is the `t` where the point lies on the circle
                // `(1-t) c0 + t c1` — a quadratic in `t`.
                let (x0, y0, r0, x1, y1, r1) = (
                    coords[0], coords[1], coords[2], coords[3], coords[4], coords[5],
                );
                let (dx, dy, dr) = (x1 - x0, y1 - y0, r1 - r0);
                let (px, py) = (x - x0, y - y0);
                let a = dx * dx + dy * dy - dr * dr;
                let b = -2.0 * (px * dx + py * dy + r0 * dr);
                let c = px * px + py * py - r0 * r0;
                let (low, high) = solve_quadratic(a, b, c)?;
                // The point lies on two circles' worth of `t`; the smaller root in
                // range is the one a reader sees, and a root outside `[0, 1]` is
                // only used when `/Extend` says the gradient may spill.
                if (0.0..=1.0).contains(&low) {
                    low
                } else if (0.0..=1.0).contains(&high) {
                    high
                } else if extend[0] && high < 0.0 {
                    0.0
                } else if extend[1] && low > 1.0 {
                    1.0
                } else {
                    return None;
                }
            }
        };
        let function = match &self.kind {
            Kind::Axial { function, .. } | Kind::Radial { function, .. } => function,
            Kind::Function { .. } => return None,
        };
        Some(self.color(&function.eval(&[t])))
    }

    /// The shading's `/Matrix`, mapping shading space to default space.
    pub(crate) fn matrix(&self) -> Matrix {
        self.matrix
    }

    /// The same shading with its matrix replaced by the full shading-to-device
    /// product, which is what `paint` needs to sample through.
    pub(crate) fn with_matrix(&self, matrix: Matrix) -> Self {
        Self {
            matrix,
            ..self.clone()
        }
    }

    fn color(&self, values: &[f64]) -> Color {
        color_in(&self.space, values)
    }
}

/// Paint a shading over the canvas, inside `mask` and `bounds` (device space).
pub(crate) fn paint(
    shading: &Shading,
    canvas: &mut Canvas,
    mask: &ClipMask,
    alpha: f64,
    mode: BlendMode,
) {
    // Every device pixel is a candidate: a shading may extend past any box the
    // operator names, and the clip decides where it lands.
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            let coverage = mask.alpha(x, y);
            if coverage <= 0.0 {
                continue;
            }
            #[allow(clippy::cast_precision_loss)]
            let point = (f64::from(x) + 0.5, f64::from(y) + 0.5);
            // Device space back into shading space for the sample.
            let Some(inverse) = shading.matrix().invert() else {
                continue;
            };
            let local = inverse.apply(crate::geom::Point::new(point.0, point.1));
            let Some(colour) = shading.colour_at(local.x, local.y) else {
                continue;
            };
            canvas.blend_with(x, y, colour, alpha * coverage, mode);
        }
    }
}

/// Resolve a colour space object, including the array forms a shading may use.
pub(crate) fn parse_space(document: &Document, object: &Object) -> Result<Space> {
    let object = match object {
        Object::Reference(id) => document.get_object(*id)?,
        other => other.clone(),
    };
    match object {
        Object::Name(name) => match name.as_slice() {
            b"DeviceGray" | b"CalGray" => Ok(Space::Gray),
            b"DeviceRGB" | b"CalRGB" => Ok(Space::Rgb),
            b"DeviceCMYK" => Ok(Space::Cmyk),
            _ => Err(Error::Unsupported {
                what: "a colour space other than Gray, RGB, CMYK, Separation or DeviceN",
            }),
        },
        Object::Array(items) => {
            let family = items
                .first()
                .and_then(Object::as_name)
                .unwrap_or(b"")
                .to_vec();
            // The family decides the shape of the array, so it is read *before*
            // any of the entries are interpreted: `[/ICCBased stream]` has no
            // alternate and no function at all, and treating every array as a
            // tint space is what made a page whose fill space is ICCBased paint
            // black — `9721 (10.9.5)`'s array sub-forms do not share a layout.
            match family.as_slice() {
                b"ICCBased" => return icc_based(document, &items),
                b"Separation" | b"DeviceN" => {}
                _ => {
                    return Err(Error::Unsupported {
                        what: "an array colour space other than ICCBased, Separation or DeviceN",
                    });
                }
            }
            // `[/Separation /Spot /DeviceCMYK fn]` and
            // `[/DeviceN [/A /B] /DeviceCMYK fn]`: the alternate space is the
            // third entry in both, and the function is the last.
            let alternate_at = 2;
            let Some(alternate) = items.get(alternate_at) else {
                return Err(Error::malformed(
                    "a colour space array without an alternate",
                ));
            };
            let alternate = Box::new(parse_space(document, alternate)?);
            let Some(tint_object) = items.last() else {
                return Err(Error::malformed("a colour space array without a function"));
            };
            let tint = Function::parse(document, tint_object)?;
            match family.as_slice() {
                b"Separation" => Ok(Space::Separation { alternate, tint }),
                b"DeviceN" => Ok(Space::DeviceN { alternate, tint }),
                _ => Err(Error::Unsupported {
                    what: "an array colour space other than ICCBased, Separation or DeviceN",
                }),
            }
        }
        _ => Err(Error::malformed(
            "a colour space that is neither a name nor an array",
        )),
    }
}

/// How many components an `/ICCBased` profile declares, and the space that means.
///
/// `/N` is the authority for the sample count (`8.6.5.5`), and the profile's own
/// numbers are what a reader without colour management can honour: 1, 3 and 4 map
/// onto Gray, RGB and CMYK. When `/N` is something else the array's `/Alternate`
/// is the writer's fallback, and it is used instead of guessing.
pub(crate) fn icc_components(document: &Document, items: &[Object]) -> Result<Space> {
    let Some(Object::Reference(id)) = items.get(1) else {
        return Err(Error::malformed("an /ICCBased space without a stream"));
    };
    // The profile is a *stream*: its `/N` lives in the stream's dictionary, which
    // `Object::as_dict` does not expose.
    let profile = document.stream(*id)?;
    match profile.dict().get(b"N").and_then(Object::as_integer) {
        Some(1) => Ok(Space::Gray),
        Some(3) => Ok(Space::Rgb),
        Some(4) => Ok(Space::Cmyk),
        _ => match profile.dict().get(b"Alternate") {
            Some(alternate) => parse_space(document, alternate),
            None => Err(Error::Unsupported {
                what: "an /ICCBased colour space of a component count it cannot name",
            }),
        },
    }
}

fn icc_based(document: &Document, items: &[Object]) -> Result<Space> {
    icc_components(document, items)
}

/// A colour in `space` from `values` (tints for `/Separation` and `/DeviceN`).
pub(crate) fn color_in(space: &Space, values: &[f64]) -> Color {
    let at = |index: usize| values.get(index).copied().unwrap_or(0.0).clamp(0.0, 1.0);
    match space {
        Space::Gray => Color::rgb(at(0), at(0), at(0)),
        Space::Rgb => Color::rgb(at(0), at(1), at(2)),
        Space::Cmyk => Color::from_cmyk(at(0), at(1), at(2), at(3)),
        Space::Separation { alternate, tint } => color_in(alternate, &tint.eval(&[at(0)])),
        Space::DeviceN { alternate, tint } => color_in(alternate, &tint.eval(values)),
    }
}

fn solve_quadratic(a: f64, b: f64, c: f64) -> Option<(f64, f64)> {
    if a.abs() < 1e-12 {
        if b.abs() < 1e-12 {
            return None;
        }
        let t = -c / b;
        return Some((t, t));
    }
    let discriminant = b * b - 4.0 * a * c;
    if discriminant < 0.0 {
        return None;
    }
    let root = discriminant.sqrt();
    let first = (-b + root) / (2.0 * a);
    let second = (-b - root) / (2.0 * a);
    Some((first.min(second), first.max(second)))
}

fn parse_function(document: &Document, object: Option<&Object>) -> Result<Function> {
    let Some(object) = object else {
        return Err(Error::malformed("a shading without a /Function"));
    };
    Function::parse(document, object)
}

fn numbers<const N: usize>(dict: &Dictionary, key: &[u8], count: usize) -> Option<[f64; N]> {
    let Object::Array(items) = dict.get(key)? else {
        return None;
    };
    let values: Vec<f64> = items.iter().filter_map(Object::as_real).collect();
    if values.len() < count || N != count {
        return None;
    }
    let mut out = [0.0; N];
    out.copy_from_slice(&values[..count]);
    Some(out)
}

fn flags(dict: &Dictionary, key: &[u8]) -> [bool; 2] {
    match dict.get(key) {
        Some(Object::Array(items)) => [
            matches!(items.first(), Some(Object::Boolean(true))),
            matches!(items.get(1), Some(Object::Boolean(true))),
        ],
        _ => [false, false],
    }
}

fn matrix_operand(items: &[Object]) -> Option<Matrix> {
    if items.len() < 6 {
        return None;
    }
    let value = |index: usize| items.get(index).and_then(Object::as_real).unwrap_or(0.0);
    Some(Matrix::new(
        value(0),
        value(1),
        value(2),
        value(3),
        value(4),
        value(5),
    ))
}

#[cfg(test)]
mod tests {
    use super::{Color, color_in, parse_space};
    use pdf2::{Document, Object};

    fn corpus(name: &str) -> Document {
        let full = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let bytes = std::fs::read(format!("{full}/corpus/tier-a/{name}")).expect("corpus");
        Document::open(bytes).expect("open")
    }

    #[test]
    fn a_separation_reaches_its_alternate_space_through_the_tint() {
        // Object 13 of the corpus page is `[/Separation /Spot /DeviceCMYK fn]`,
        // whose tint function runs to `[0.1 0.9 0.8 0.05]` at full ink.
        let document = corpus("function_based_shading_cmyk.pdf");
        let space = parse_space(&document, &Object::Reference(pdf2::ObjectId::new(13, 0)))
            .expect("separation space");
        let none = color_in(&space, &[0.0]).to_bytes();
        let full = color_in(&space, &[1.0]).to_bytes();
        assert_eq!(none, [255, 255, 255], "no ink is white");
        assert_eq!(full, Color::from_cmyk(0.1, 0.9, 0.8, 0.05).to_bytes());
        let half = color_in(&space, &[0.5]).to_bytes();
        assert_ne!(half, none);
        assert_ne!(half, full);
    }

    #[test]
    fn a_devicen_space_feeds_its_function_every_tint() {
        // `[/DeviceN [/A /B] /DeviceCMYK fn]` with one input per colourant.
        let document = corpus("function_based_shading_cmyk.pdf");
        let space = parse_space(
            &document,
            &Object::Array(vec![
                Object::name("DeviceN"),
                Object::Array(vec![Object::name("A"), Object::name("B")]),
                Object::name("DeviceCMYK"),
                Object::Reference(pdf2::ObjectId::new(12, 0)),
            ]),
        )
        .expect("devicen space");
        // Object 12 is `C0 [0 0 0 0] → C1 [0.1 0.9 0.8 0.05]` over one input.
        assert_eq!(
            color_in(&space, &[1.0, 0.0]).to_bytes(),
            Color::from_cmyk(0.1, 0.9, 0.8, 0.05).to_bytes()
        );
    }
}

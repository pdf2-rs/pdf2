//! Scanline rasterisation: filling polygons into the canvas through a clip mask,
//! and turning polygons into a clip mask.

use crate::canvas::{BlendMode, Canvas, ClipMask, Color};
use crate::geom::Point;

/// One polygon edge, with the winding direction it contributes.
struct Edge {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
    winding: i32,
}

fn edges_of(polygons: &[Vec<Point>]) -> Vec<Edge> {
    let mut edges = Vec::new();
    for polygon in polygons {
        for index in 0..polygon.len() {
            let start = polygon[index];
            let end = polygon[(index + 1) % polygon.len()];
            if (start.y - end.y).abs() < f64::EPSILON {
                continue;
            }
            let winding = if end.y > start.y { 1 } else { -1 };
            edges.push(Edge {
                x0: start.x,
                y0: start.y,
                x1: end.x,
                y1: end.y,
                winding,
            });
        }
    }
    edges
}

/// Horizontal spans of a scanline in device x, fractional at the ends.
///
/// Anti-aliasing needs the *fraction* of a pixel a span covers, so this returns
/// the exact crossing positions rather than rounding them; [`pixel_range`] does
/// the rounding for the clip mask, which is a boolean by nature.
fn spans_of(edges: &[Edge], row: f64, even_odd: bool) -> Vec<(f64, f64)> {
    let mut crossings: Vec<(f64, i32)> = Vec::new();
    for edge in edges {
        let low = edge.y0.min(edge.y1);
        let high = edge.y0.max(edge.y1);
        if row < low || row >= high {
            continue;
        }
        let t = (row - edge.y0) / (edge.y1 - edge.y0);
        crossings.push((edge.x0 + t * (edge.x1 - edge.x0), edge.winding));
    }
    if crossings.is_empty() {
        return Vec::new();
    }
    crossings.sort_by(|left, right| left.0.total_cmp(&right.0));

    let mut result = Vec::new();
    let mut winding = 0;
    let mut start_x: Option<f64> = None;
    for (x, direction) in crossings {
        let was_inside = if even_odd {
            winding & 1 != 0
        } else {
            winding != 0
        };
        if even_odd {
            winding += 1;
        } else {
            winding += direction;
        }
        let is_inside = if even_odd {
            winding & 1 != 0
        } else {
            winding != 0
        };
        match (was_inside, is_inside) {
            (false, true) => start_x = Some(x),
            (true, false) => {
                if let Some(start) = start_x.take()
                    && x > start
                {
                    result.push((start, x));
                }
            }
            _ => {}
        }
    }
    if let Some(start) = start_x {
        result.push((start, start));
    }
    result
}

/// Device x range a fractional span touches, as `[first, past)`.
fn span_pixels(start: f64, end: f64) -> (i64, i64) {
    #[allow(clippy::cast_possible_truncation)]
    let first = start.floor() as i64;
    #[allow(clippy::cast_possible_truncation)]
    let past = end.ceil() as i64;
    (first, past.max(first))
}

/// Sub-scanlines per device row: 1 would be no anti-aliasing, 4 is enough to
/// take the staircase off a glyph edge at normal sizes, and the horizontal
/// coverage is exact, so the error is vertical only.
const AA_SUBROWS: usize = 4;

#[allow(clippy::cast_possible_truncation)]
fn x_i32(x: i64) -> i32 {
    x as i32
}

/// Fill polygons with `color`, restricted to `mask`.
///
/// `even_odd` selects the even-odd rule; otherwise the non-zero winding rule
/// applies. The scanline is sampled at pixel centres, so results are crisp and
/// deterministic — no anti-aliasing yet.
pub fn fill_polygons(
    canvas: &mut Canvas,
    mask: &ClipMask,
    polygons: &[Vec<Point>],
    color: Color,
    even_odd: bool,
    alpha: f64,
    mode: BlendMode,
) {
    if polygons.is_empty() {
        return;
    }
    let edges = edges_of(polygons);
    if edges.is_empty() {
        return;
    }
    // Only the rows the outline touches: a glyph is a dozen rows tall, and
    // scanning the whole canvas for each of the two thousand glyphs on a page of
    // prose was most of that page's render time.
    let (low, high) = edges
        .iter()
        .fold((f64::MAX, f64::MIN), |(low, high), edge| {
            (
                low.min(edge.y0.min(edge.y1)),
                high.max(edge.y0.max(edge.y1)),
            )
        });
    if high <= low {
        return;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let first_row = ((low - 0.5).ceil().max(0.0) as u32).min(canvas.height());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let past_last = ((high - 0.5).ceil().max(0.0) as u32).min(canvas.height());
    for y in first_row..past_last {
        // Four sub-scanlines per device row, each contributing an exact
        // horizontal fraction, summed and blended **once** per pixel: blending
        // four times at a quarter alpha is not the same thing — it lands on
        // `1-(1-a/4)^4`, about 68 % instead of 100 %.
        let mut sub_spans: [Vec<(f64, f64)>; AA_SUBROWS] = Default::default();
        let mut low_x = i64::MAX;
        let mut high_x = i64::MIN;
        for (sub, spans) in sub_spans.iter_mut().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let offset = (sub as f64 + 0.5) / AA_SUBROWS as f64;
            *spans = spans_of(&edges, f64::from(y) + offset, even_odd);
            for (start, end) in spans.iter() {
                let (first, past) = span_pixels(*start, *end);
                low_x = low_x.min(first);
                high_x = high_x.max(past);
            }
        }
        if low_x >= high_x {
            continue;
        }
        let mut coverage = vec![0.0_f64; usize::try_from(high_x - low_x).unwrap_or(0)];
        for spans in &sub_spans {
            for (start, end) in spans {
                let (first, past) = span_pixels(*start, *end);
                for x in first.max(low_x)..past.min(high_x) {
                    #[allow(clippy::cast_precision_loss)]
                    let left = (x as f64).max(*start);
                    #[allow(clippy::cast_precision_loss)]
                    let right = ((x + 1) as f64).min(*end);
                    if right > left {
                        let index = usize::try_from(x - low_x).unwrap_or(0);
                        coverage[index] += (right - left).min(1.0);
                    }
                }
            }
        }
        for (index, covered) in coverage.iter().enumerate() {
            if *covered <= 0.0 {
                continue;
            }
            let Some(x) = low_x.checked_add(i64::try_from(index).unwrap_or(0)) else {
                continue;
            };
            if x < 0 || x >= i64::from(canvas.width()) {
                continue;
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let pixel_x = x as u32;
            let mask_coverage = mask.alpha(pixel_x, y);
            if mask_coverage <= 0.0 {
                continue;
            }
            let fraction = (covered / AA_SUBROWS as f64).min(1.0);
            canvas.blend_with(pixel_x, y, color, alpha * mask_coverage * fraction, mode);
        }
    }
}

/// Rasterise polygons into a clip mask of the given size: inside where the
/// fill rule says painted, outside everywhere else.
pub fn clip_mask(width: u32, height: u32, polygons: &[Vec<Point>], even_odd: bool) -> ClipMask {
    let mut mask = ClipMask::none(width, height);
    let edges = edges_of(polygons);
    for y in 0..height {
        let row = f64::from(y) + 0.5;
        for (start, end) in spans_of(&edges, row, even_odd) {
            let (first, past) = span_pixels(start, end);
            let from = first.max(0);
            let to = past.min(i64::from(width));
            for x in from..to {
                // A pixel belongs to the clip when its centre is covered.
                let centre = f64::from(x_i32(x)) + 0.5;
                if start <= centre && centre < end {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                    mask.set(x as u32, y, true);
                }
            }
        }
    }
    mask
}

#[cfg(test)]
mod tests {
    use super::{clip_mask, edges_of, fill_polygons, spans_of};
    use crate::canvas::{BlendMode, Canvas, ClipMask, Color};
    use crate::geom::Point;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Vec<Point> {
        vec![
            Point::new(x, y),
            Point::new(x + width, y),
            Point::new(x + width, y + height),
            Point::new(x, y + height),
        ]
    }

    #[test]
    fn a_rectangle_covers_its_centre_pixels() {
        let mut canvas = Canvas::new(10, 10, Color::WHITE);
        let mask = ClipMask::all(10, 10);
        fill_polygons(
            &mut canvas,
            &mask,
            &[rect(2.0, 3.0, 4.0, 2.0)],
            Color::BLACK,
            false,
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(canvas.pixel(3, 4), Some([0, 0, 0, 255]));
        assert_eq!(canvas.pixel(1, 4), Some([255, 255, 255, 255]));
        assert_eq!(canvas.pixel(3, 2), Some([255, 255, 255, 255]));
        assert_eq!(canvas.pixel(6, 4), Some([255, 255, 255, 255]));
    }

    #[test]
    fn the_even_odd_rule_cuts_the_overlap() {
        let polygons = [rect(0.0, 0.0, 6.0, 6.0), rect(2.0, 2.0, 6.0, 6.0)];
        let mut nonzero = Canvas::new(10, 10, Color::WHITE);
        fill_polygons(
            &mut nonzero,
            &ClipMask::all(10, 10),
            &polygons,
            Color::BLACK,
            false,
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(nonzero.pixel(3, 3), Some([0, 0, 0, 255]));

        let mut even_odd = Canvas::new(10, 10, Color::WHITE);
        fill_polygons(
            &mut even_odd,
            &ClipMask::all(10, 10),
            &polygons,
            Color::BLACK,
            true,
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(even_odd.pixel(3, 3), Some([255, 255, 255, 255]));
        assert_eq!(even_odd.pixel(1, 1), Some([0, 0, 0, 255]));
    }

    #[test]
    fn the_mask_restricts_painting() {
        let mut mask = ClipMask::all(10, 10);
        for x in 0..5 {
            mask.set(x, 0, false);
        }
        let mut canvas = Canvas::new(10, 10, Color::WHITE);
        fill_polygons(
            &mut canvas,
            &mask,
            &[rect(0.0, 0.0, 10.0, 1.0)],
            Color::BLACK,
            false,
            1.0,
            BlendMode::Normal,
        );
        assert_eq!(canvas.pixel(0, 0), Some([255, 255, 255, 255]));
        assert_eq!(canvas.pixel(6, 0), Some([0, 0, 0, 255]));
    }

    #[test]
    fn a_clip_mask_covers_exactly_the_clip_path() {
        let mask = clip_mask(10, 10, &[rect(2.0, 2.0, 3.0, 3.0)], false);
        assert!(mask.contains(3, 3));
        assert!(!mask.contains(1, 1));
        assert!(!mask.contains(6, 6));
        assert_eq!(mask.count(), 9);
    }

    #[test]
    fn spans_handle_shared_edges() {
        let edges = edges_of(&[rect(0.0, 0.0, 4.0, 4.0)]);
        let row = spans_of(&edges, 2.5, false);
        assert_eq!(row, vec![(0.0, 4.0)]);
        assert!(spans_of(&edges, 5.5, false).is_empty());
    }

    #[test]
    fn an_edge_through_a_pixel_paints_half_of_it() {
        // A rectangle whose right edge is at x = 3.5 covers half of pixel 3: with
        // four sub-scanlines the blend composes to about 50 % grey.
        let mut canvas = Canvas::new(8, 4, Color::WHITE);
        fill_polygons(
            &mut canvas,
            &ClipMask::all(8, 4),
            &[rect(1.0, 1.0, 2.5, 2.0)],
            Color::BLACK,
            false,
            1.0,
            BlendMode::Normal,
        );
        let partial = canvas.pixel(3, 2).expect("inside");
        assert!(
            (120..=135).contains(&u16::from(partial[0])),
            "half coverage is about 128, got {partial:?}"
        );
        assert_eq!(canvas.pixel(2, 2), Some([0, 0, 0, 255]), "fully inside");
        assert_eq!(canvas.pixel(4, 2), Some([255, 255, 255, 255]), "outside");
    }

    #[test]
    fn an_edge_on_a_pixel_boundary_leaves_no_half_pixels() {
        let mut canvas = Canvas::new(8, 4, Color::WHITE);
        fill_polygons(
            &mut canvas,
            &ClipMask::all(8, 4),
            &[rect(1.0, 1.0, 3.0, 2.0)],
            Color::BLACK,
            false,
            1.0,
            BlendMode::Normal,
        );
        for x in 1..4 {
            assert_eq!(canvas.pixel(x, 2), Some([0, 0, 0, 255]), "pixel {x}");
        }
        assert_eq!(canvas.pixel(0, 2), Some([255, 255, 255, 255]));
        assert_eq!(canvas.pixel(4, 2), Some([255, 255, 255, 255]));
    }
}

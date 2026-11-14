//! Paths: construction in user space, flattening to device-space polylines,
//! and turning a stroked polyline into the polygons a fill can paint.

use crate::geom::{Matrix, Point};

/// Line-dash pattern, in user-space lengths.
#[derive(Debug, Clone, PartialEq)]
pub struct Dash {
    /// Alternating on/off lengths; empty means a solid line.
    pub pattern: Vec<f64>,
    /// Distance into the pattern at which the path starts.
    pub phase: f64,
}

impl Dash {
    /// A solid line.
    pub fn solid() -> Self {
        Self {
            pattern: Vec::new(),
            phase: 0.0,
        }
    }

    /// Whether the line is solid.
    pub fn is_solid(&self) -> bool {
        self.pattern.is_empty() || self.pattern.iter().all(|length| *length <= 0.0)
    }
}

/// How a stroked path ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    /// Ends at the endpoint.
    Butt,
    /// Rounded end, a disc of half the line width.
    Round,
    /// Extends by half the line width.
    Square,
}

/// How stroked segments meet, from the `j` operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// A sharp join, cut back to a bevel when the miter limit is exceeded.
    Miter,
    /// A rounded join.
    Round,
    /// A cut-off join.
    Bevel,
}

/// One segment of a subpath.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    /// A straight line to the point.
    Line(Point),
    /// A cubic Bézier curve.
    Cubic {
        /// First control point.
        c1: Point,
        /// Second control point.
        c2: Point,
        /// End point.
        to: Point,
    },
}

/// A connected run of segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Subpath {
    /// Where the subpath starts.
    pub start: Point,
    /// The segments that follow.
    pub segments: Vec<Segment>,
    /// Whether the subpath was closed.
    pub closed: bool,
}

/// A path in user space.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Path {
    /// The subpaths, in the order they were added.
    pub subpaths: Vec<Subpath>,
}

impl Path {
    /// An empty path.
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the path has no segments.
    pub fn is_empty(&self) -> bool {
        self.subpaths
            .iter()
            .all(|subpath| subpath.segments.is_empty())
    }

    /// Start a new subpath.
    pub fn move_to(&mut self, point: Point) {
        self.subpaths.push(Subpath {
            start: point,
            segments: Vec::new(),
            closed: false,
        });
    }

    /// A line from the current point.
    pub fn line_to(&mut self, point: Point) {
        if let Some(subpath) = self.subpaths.last_mut() {
            subpath.segments.push(Segment::Line(point));
        } else {
            self.move_to(point);
        }
    }

    /// A cubic curve from the current point.
    pub fn curve_to(&mut self, c1: Point, c2: Point, to: Point) {
        if let Some(subpath) = self.subpaths.last_mut() {
            subpath.segments.push(Segment::Cubic { c1, c2, to });
        } else {
            self.move_to(to);
        }
    }

    /// Close the current subpath.
    pub fn close(&mut self) {
        if let Some(subpath) = self.subpaths.last_mut() {
            subpath.closed = true;
        }
    }

    /// Append a rectangle as its own closed subpath.
    pub fn add_rect(&mut self, x: f64, y: f64, width: f64, height: f64) {
        self.move_to(Point::new(x, y));
        self.line_to(Point::new(x + width, y));
        self.line_to(Point::new(x + width, y + height));
        self.line_to(Point::new(x, y + height));
        self.close();
    }

    /// Flatten to device-space polylines, applying `ctm`.
    ///
    /// Curves are subdivided a fixed number of times: enough that the fixtures
    /// are visibly round at 1× and cheap to rasterise, and documented as the
    /// tolerance model until adaptive flattening replaces it.
    pub fn flatten(&self, ctm: Matrix) -> Vec<Vec<Point>> {
        const CURVE_STEPS: usize = 24;
        let mut polylines = Vec::with_capacity(self.subpaths.len());
        for subpath in &self.subpaths {
            let mut points = vec![ctm.apply(subpath.start)];
            let mut current = subpath.start;
            for segment in &subpath.segments {
                match *segment {
                    Segment::Line(to) => {
                        points.push(ctm.apply(to));
                        current = to;
                    }
                    Segment::Cubic { c1, c2, to } => {
                        for step in 1..=CURVE_STEPS {
                            let t = step as f64 / CURVE_STEPS as f64;
                            points.push(ctm.apply(cubic_at(current, c1, c2, to, t)));
                        }
                        current = to;
                    }
                }
            }
            if subpath.closed && points.len() > 1 {
                let first = points[0];
                let last = *points.last().expect("non-empty");
                if (first.x - last.x).abs() > f64::EPSILON
                    || (first.y - last.y).abs() > f64::EPSILON
                {
                    points.push(first);
                }
            }
            if points.len() > 1 {
                polylines.push(points);
            }
        }
        polylines
    }
}

fn cubic_at(p0: Point, c1: Point, c2: Point, p3: Point, t: f64) -> Point {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    Point::new(
        a * p0.x + b * c1.x + c * c2.x + d * p3.x,
        a * p0.y + b * c1.y + c * c2.y + d * p3.y,
    )
}

/// Split a device-space polyline into the "on" runs of a dash pattern.
///
/// Dash lengths are user-space values; they are scaled by the square root of
/// the CTM determinant, which matches a uniform scale exactly and is the usual
/// approximation under shear.
pub fn dash_polyline(polyline: &[Point], dash: &Dash, ctm_scale: f64) -> Vec<Vec<Point>> {
    if dash.is_solid() || polyline.len() < 2 {
        return vec![polyline.to_vec()];
    }
    let pattern: Vec<f64> = dash
        .pattern
        .iter()
        .map(|length| (length * ctm_scale).abs())
        .filter(|length| *length > 1e-9)
        .collect();
    if pattern.is_empty() {
        return vec![polyline.to_vec()];
    }
    let mut runs = Vec::new();
    let mut current_run: Vec<Point> = Vec::new();
    // Which pattern entry applies, and how far into it we are.
    let mut index = 0usize;
    let mut remaining = pattern[0];
    let mut phase = dash.phase * ctm_scale;
    while phase > 0.0 {
        if phase >= remaining {
            phase -= remaining;
            index = (index + 1) % pattern.len();
            remaining = pattern[index];
        } else {
            remaining -= phase;
            phase = 0.0;
        }
    }
    let mut on = index.is_multiple_of(2);
    if on {
        current_run.push(polyline[0]);
    }
    for window in polyline.windows(2) {
        let mut start = window[0];
        let end = window[1];
        let dx = end.x - start.x;
        let dy = end.y - start.y;
        let length = (dx * dx + dy * dy).sqrt();
        if length <= 1e-12 {
            continue;
        }
        let unit_x = dx / length;
        let unit_y = dy / length;
        let mut travelled = 0.0;
        while travelled < length {
            let step = remaining.min(length - travelled);
            let next = Point::new(start.x + unit_x * step, start.y + unit_y * step);
            if on {
                if current_run.is_empty() {
                    current_run.push(start);
                }
                current_run.push(next);
            }
            travelled += step;
            remaining -= step;
            if remaining <= 1e-9 {
                if on && current_run.len() > 1 {
                    runs.push(std::mem::take(&mut current_run));
                }
                index = (index + 1) % pattern.len();
                remaining = pattern[index];
                on = index.is_multiple_of(2);
            }
            start = next;
        }
    }
    if on && current_run.len() > 1 {
        runs.push(current_run);
    }
    runs
}

/// Turn stroked polylines into filled polygons.
///
/// The union of per-segment quads, round joins and caps is returned as
/// polygons wound the same way, so a non-zero fill paints exactly the stroke.
pub fn stroke_outline(
    polylines: &[Vec<Point>],
    width: f64,
    cap: Cap,
    join: Join,
    miter_limit: f64,
) -> Vec<Vec<Point>> {
    let half = (width / 2.0).max(0.0);
    if half <= 0.0 {
        return Vec::new();
    }
    let mut polygons = Vec::new();
    for polyline in polylines {
        if polyline.len() < 2 {
            continue;
        }
        let last_index = polyline.len() - 2;
        for (index, window) in polyline.windows(2).enumerate() {
            let start = window[0];
            let end = window[1];
            let first = index == 0;
            let last = index == last_index;
            // Square caps extend the end segments by half the line width;
            // butt caps stop exactly at the endpoint.
            let (from, to) = if cap == Cap::Square {
                (
                    if first {
                        extend(start, end, half)
                    } else {
                        start
                    },
                    if last { extend(end, start, half) } else { end },
                )
            } else {
                (start, end)
            };
            let quad = segment_quad(from, to, half);
            if quad.is_empty() {
                continue;
            }
            polygons.push(oriented(quad));
            // A vertex between two segments joins them; the closing vertex of a
            // closed path is a seam, covered by the first segment's quad.
            if !first {
                polygons.push(join_polygon(polyline, index, half, join, miter_limit));
            }
            if last && cap == Cap::Round {
                polygons.push(oriented(circle(end, half)));
            }
        }
        if cap == Cap::Round {
            polygons.push(oriented(circle(polyline[0], half)));
        }
    }
    polygons
}

/// The wedge that joins the segment ending at `polyline[index]` to the one
/// starting there.
fn join_polygon(
    polyline: &[Point],
    index: usize,
    half: f64,
    join: Join,
    miter_limit: f64,
) -> Vec<Point> {
    let vertex = polyline[index];
    let before = polyline[index - 1];
    let after = polyline[index + 1];
    if join == Join::Round {
        return oriented(circle(vertex, half));
    }
    let (Some(incoming), Some(outgoing)) = (unit(before, vertex), unit(vertex, after)) else {
        return oriented(circle(vertex, half));
    };
    // The outer side of a turn is the side the path turns away from: opposite
    // the left normal for a left turn, along it for a right turn.
    let cross = incoming.0 * outgoing.1 - incoming.1 * outgoing.0;
    let factor = if cross > 0.0 { -1.0 } else { 1.0 };
    let back_out = offset(vertex, incoming, half * factor);
    let forward_out = offset(vertex, outgoing, half * factor);
    if join == Join::Bevel || cross.abs() < 1e-9 {
        return oriented(vec![vertex, back_out, forward_out]);
    }
    // Miter: the two outer edges meet at a point along the bisector; past the
    // limit the standard cuts the corner instead (8.4.3.5).
    let Some(tip) = intersect(back_out, incoming, forward_out, outgoing) else {
        return oriented(vec![vertex, back_out, forward_out]);
    };
    let length = ((tip.x - vertex.x).powi(2) + (tip.y - vertex.y).powi(2)).sqrt();
    if length > half * miter_limit.max(1.0) {
        return oriented(vec![vertex, back_out, forward_out]);
    }
    oriented(vec![vertex, back_out, tip, forward_out])
}

/// The point `distance` along the left normal of `direction` from `from`.
fn offset(from: Point, direction: (f64, f64), distance: f64) -> Point {
    Point::new(
        from.x - direction.1 * distance,
        from.y + direction.0 * distance,
    )
}

/// A unit vector in the direction `to - from`, or `None` for a zero length.
fn unit(from: Point, to: Point) -> Option<(f64, f64)> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 1e-12 {
        return None;
    }
    Some((dx / length, dy / length))
}

/// Where the line through `p` along `d` meets the line through `q` along `e`.
fn intersect(p: Point, d: (f64, f64), q: Point, e: (f64, f64)) -> Option<Point> {
    let denominator = d.0 * e.1 - d.1 * e.0;
    if denominator.abs() < 1e-9 {
        return None;
    }
    let dx = q.x - p.x;
    let dy = q.y - p.y;
    let t = (dx * e.1 - dy * e.0) / denominator;
    Some(Point::new(p.x + d.0 * t, p.y + d.1 * t))
}

/// A point `distance` away from `from`, in the direction `from → towards`.
fn extend(from: Point, towards: Point, distance: f64) -> Point {
    let dx = towards.x - from.x;
    let dy = towards.y - from.y;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 1e-12 {
        return from;
    }
    Point::new(
        from.x - dx / length * distance,
        from.y - dy / length * distance,
    )
}

fn segment_quad(start: Point, end: Point, half: f64) -> Vec<Point> {
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= 1e-12 {
        return Vec::new();
    }
    let nx = -dy / length * half;
    let ny = dx / length * half;
    vec![
        Point::new(start.x + nx, start.y + ny),
        Point::new(end.x + nx, end.y + ny),
        Point::new(end.x - nx, end.y - ny),
        Point::new(start.x - nx, start.y - ny),
    ]
}

/// A polygon approximating a disc.
pub fn circle(center: Point, radius: f64) -> Vec<Point> {
    const SEGMENTS: usize = 16;
    let mut points = Vec::with_capacity(SEGMENTS);
    for step in 0..SEGMENTS {
        let angle = step as f64 / SEGMENTS as f64 * std::f64::consts::TAU;
        points.push(Point::new(
            center.x + radius * angle.cos(),
            center.y + radius * angle.sin(),
        ));
    }
    points
}

/// Reverse a polygon when it is wound clockwise, so unions behave under the
/// non-zero rule.
fn oriented(mut polygon: Vec<Point>) -> Vec<Point> {
    let area = signed_area(&polygon);
    if area < 0.0 {
        polygon.reverse();
    }
    polygon
}

fn signed_area(polygon: &[Point]) -> f64 {
    let mut sum = 0.0;
    for index in 0..polygon.len() {
        let current = polygon[index];
        let next = polygon[(index + 1) % polygon.len()];
        sum += current.x * next.y - next.x * current.y;
    }
    sum / 2.0
}

#[cfg(test)]
mod tests {
    use super::{dash_polyline, stroke_outline, Cap, Dash, Join, Path};
    use crate::geom::{Matrix, Point};

    #[test]
    fn rectangles_flatten_to_closed_polylines() {
        let mut path = Path::new();
        path.add_rect(0.0, 0.0, 10.0, 20.0);
        let polylines = path.flatten(Matrix::IDENTITY);
        assert_eq!(polylines.len(), 1);
        assert_eq!(polylines[0].len(), 5, "four corners plus the closing point");
        assert_eq!(polylines[0][0], Point::new(0.0, 0.0));
        assert_eq!(polylines[0][2], Point::new(10.0, 20.0));
    }

    #[test]
    fn curves_are_subdivided() {
        let mut path = Path::new();
        path.move_to(Point::new(0.0, 0.0));
        path.curve_to(
            Point::new(0.0, 10.0),
            Point::new(10.0, 10.0),
            Point::new(10.0, 0.0),
        );
        let polylines = path.flatten(Matrix::IDENTITY);
        assert_eq!(polylines[0].len(), 25);
        assert_eq!(*polylines[0].last().expect("end"), Point::new(10.0, 0.0));
    }

    #[test]
    fn a_solid_dash_keeps_the_polyline() {
        let line = vec![Point::new(0.0, 0.0), Point::new(10.0, 0.0)];
        let runs = dash_polyline(&line, &Dash::solid(), 1.0);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0], line);
    }

    #[test]
    fn a_dash_pattern_splits_the_line() {
        let line = vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0)];
        let dash = Dash {
            pattern: vec![5.0, 5.0],
            phase: 0.0,
        };
        let runs = dash_polyline(&line, &dash, 1.0);
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0], vec![Point::new(0.0, 0.0), Point::new(5.0, 0.0)]);
        assert_eq!(runs[1], vec![Point::new(10.0, 0.0), Point::new(15.0, 0.0)]);
    }

    #[test]
    fn a_phase_offsets_the_first_run() {
        let line = vec![Point::new(0.0, 0.0), Point::new(20.0, 0.0)];
        let dash = Dash {
            pattern: vec![5.0, 5.0],
            phase: 2.0,
        };
        let runs = dash_polyline(&line, &dash, 1.0);
        assert_eq!(runs[0][0], Point::new(0.0, 0.0));
        assert_eq!(runs[0][1], Point::new(3.0, 0.0));
    }

    #[test]
    fn strokes_become_polygons_around_the_line() {
        let line = vec![Point::new(10.0, 10.0), Point::new(30.0, 10.0)];
        let polygons = stroke_outline(&[line], 4.0, Cap::Butt, Join::Miter, 10.0);
        assert!(!polygons.is_empty());
        // Every polygon is wound counter-clockwise after orientation.
        for polygon in &polygons {
            assert!(super::signed_area(polygon) >= 0.0);
        }
        // The quad around the segment spans y from 8 to 12.
        let quad = &polygons[0];
        let min_y = quad.iter().fold(f64::MAX, |min, point| min.min(point.y));
        let max_y = quad.iter().fold(f64::MIN, |max, point| max.max(point.y));
        assert!((min_y - 8.0).abs() < 1e-9, "{min_y}");
        assert!((max_y - 12.0).abs() < 1e-9, "{max_y}");
    }

    /// A right-angle corner from (0, 10) to (10, 10) to (10, 20) with a 4-wide
    /// stroke: the outer corner is the square [10, 12] x [8, 10], and a 90°
    /// miter needs 1.41 half-widths to reach into it.
    fn corner(join: Join, limit: f64) -> Vec<Vec<Point>> {
        let path = vec![
            Point::new(0.0, 10.0),
            Point::new(10.0, 10.0),
            Point::new(10.0, 20.0),
        ];
        stroke_outline(&[path], 4.0, Cap::Butt, join, limit)
    }

    fn painted(polygons: &[Vec<Point>], x: u32, y: u32) -> bool {
        let mut canvas = crate::canvas::Canvas::new(24, 30, crate::canvas::Color::WHITE);
        let mask = crate::canvas::ClipMask::all(24, 30);
        crate::raster::fill_polygons(
            &mut canvas,
            &mask,
            polygons,
            crate::canvas::Color::BLACK,
            false,
            1.0,
        );
        canvas.pixel(x, y) == Some([0, 0, 0, 255])
    }

    #[test]
    fn a_miter_join_fills_the_corner_and_a_bevel_cuts_it() {
        // (11, 8) is inside the corner square but outside both stroke bands.
        assert!(
            painted(&corner(Join::Miter, 10.0), 11, 8),
            "the miter fills the corner"
        );
        assert!(
            !painted(&corner(Join::Bevel, 10.0), 11, 8),
            "the bevel cuts the corner"
        );
    }

    #[test]
    fn the_miter_limit_cuts_the_corner_back_to_a_bevel() {
        // The 90-degree miter is 1.41 half-widths long: accepted at the default
        // limit of 10, cut back once the limit is under 1.41 (8.4.3.5).
        assert!(
            painted(&corner(Join::Miter, 10.0), 11, 8),
            "limit 10 miters"
        );
        assert!(
            !painted(&corner(Join::Miter, 1.2), 11, 8),
            "limit 1.2 bevels"
        );
    }
}

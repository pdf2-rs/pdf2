//! Geometry: points and the 2×3 affine matrix PDF uses for the CTM.

/// A point in user or device space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// Horizontal coordinate.
    pub x: f64,
    /// Vertical coordinate.
    pub y: f64,
}

impl Point {
    /// A point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A 2×3 affine transform, in the order PDF writes it: `a b c d e f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrix {
    /// x scale.
    pub a: f64,
    /// x shear.
    pub b: f64,
    /// y shear.
    pub c: f64,
    /// y scale.
    pub d: f64,
    /// x translation.
    pub e: f64,
    /// y translation.
    pub f: f64,
}

impl Matrix {
    /// The identity.
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: 0.0,
    };

    /// A matrix from its six values.
    pub const fn new(a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> Self {
        Self { a, b, c, d, e, f }
    }

    /// `self × other`: a point is transformed by `self` first, then by
    /// `other` — the order PDF's `cm` operator composes in (`cm` is applied
    /// before the existing CTM).
    pub fn multiply(self, other: Self) -> Self {
        Self {
            a: self.a * other.a + self.b * other.c,
            b: self.a * other.b + self.b * other.d,
            c: self.c * other.a + self.d * other.c,
            d: self.c * other.b + self.d * other.d,
            e: self.e * other.a + self.f * other.c + other.e,
            f: self.e * other.b + self.f * other.d + other.f,
        }
    }

    /// Transform a point.
    pub fn apply(self, point: Point) -> Point {
        Point {
            x: self.a * point.x + self.c * point.y + self.e,
            y: self.b * point.x + self.d * point.y + self.f,
        }
    }

    /// The inverse, when the matrix is not singular.
    pub fn invert(self) -> Option<Self> {
        let determinant = self.a * self.d - self.b * self.c;
        if determinant.abs() < 1e-12 {
            return None;
        }
        let scale = 1.0 / determinant;
        Some(Self {
            a: self.d * scale,
            b: -self.b * scale,
            c: -self.c * scale,
            d: self.a * scale,
            e: (self.c * self.f - self.d * self.e) * scale,
            f: (self.b * self.e - self.a * self.f) * scale,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Matrix, Point};

    #[test]
    fn the_identity_leaves_points_alone() {
        assert_eq!(
            Matrix::IDENTITY.apply(Point::new(3.0, -4.0)),
            Point::new(3.0, -4.0)
        );
    }

    #[test]
    fn multiplication_applies_the_first_matrix_first() {
        let translate = Matrix::new(1.0, 0.0, 0.0, 1.0, 10.0, 0.0);
        let scale = Matrix::new(2.0, 0.0, 0.0, 2.0, 0.0, 0.0);
        // translate · scale: (1, 0) → translate → (11, 0) → scale → (22, 0).
        assert_eq!(
            translate.multiply(scale).apply(Point::new(1.0, 0.0)),
            Point::new(22.0, 0.0)
        );
        // scale · translate: (1, 0) → scale → (2, 0) → translate → (12, 0).
        assert_eq!(
            scale.multiply(translate).apply(Point::new(1.0, 0.0)),
            Point::new(12.0, 0.0)
        );
    }

    #[test]
    fn inversion_round_trips_a_point() {
        let matrix = Matrix::new(2.0, 0.5, -0.5, 3.0, 7.0, -9.0);
        let inverse = matrix.invert().expect("invertible");
        let point = Point::new(4.0, 5.0);
        let back = inverse.apply(matrix.apply(point));
        assert!((back.x - point.x).abs() < 1e-9 && (back.y - point.y).abs() < 1e-9);
    }

    #[test]
    fn singular_matrices_have_no_inverse() {
        assert!(Matrix::new(0.0, 0.0, 0.0, 0.0, 1.0, 1.0).invert().is_none());
    }
}

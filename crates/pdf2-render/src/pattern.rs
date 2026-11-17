//! Tiling patterns (`8.7.3`, `/PatternType 1`): the cell's dictionary.
//!
//! A tiling pattern replicates one **cell** over the filled area. Its stream
//! dictionary (Table 73) names the cell's box, the step between copies, how the
//! cell's colour is decided, and the content stream that paints one instance.
//! This module reads that dictionary, so the walker can say `pattern` — the page
//! reports the operator instead of painting the previous fill colour — while the
//! lattice that replicates the cell is the next round's work.
//!
//! Three things the clause is explicit about and this parser keeps separate:
//! `/BBox` clips the cell and `/XStep`/`/YStep` are the lattice vectors, which
//! "may be either positive or negative but shall not be zero"; `/PaintType` 1
//! paints with the cell's own colour while 2 is a stencil poured with the colour
//! given at use time; and `/TilingType` is a device-pixel spacing hint, not a
//! geometry change.

// The cell's keys are read here and the lattice that uses them lands in the next
// round; the fields are kept from the parse rather than re-derived later.
#![allow(dead_code)]

use pdf2::{Dictionary, Document, Object, Stream};

use crate::geom::Matrix;
use crate::{Error, Result};

/// How a cell's colour is decided (`/PaintType`, Table 73).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaintType {
    /// Type 1: the content stream paints the cell's colours.
    Coloured,
    /// Type 2: the cell is a stencil, poured with the colour given at use time.
    Uncoloured,
}

/// A parsed pattern cell: the dictionary, and the content that paints one copy.
#[derive(Debug, Clone)]
pub(crate) struct Tile {
    /// `/BBox` in pattern space: the clip for one cell.
    pub(crate) bbox: [f64; 4],
    /// `/Matrix`: pattern space to the parent's default space.
    pub(crate) matrix: Matrix,
    /// `/XStep`, never zero.
    pub(crate) x_step: f64,
    /// `/YStep`, never zero.
    pub(crate) y_step: f64,
    pub(crate) paint_type: PaintType,
    /// `/TilingType` 1–3: a spacing hint for the device grid.
    pub(crate) tiling_type: i64,
    /// The cell's own resources, when it declares any.
    pub(crate) resources: Option<Dictionary>,
    /// The decoded content stream that paints one cell.
    pub(crate) content: Vec<u8>,
}

impl Tile {
    /// Read a pattern stream's dictionary (Table 73) and decode its content.
    ///
    /// A dictionary missing a required key, or with a zero step, is an error:
    /// the caller reports `pattern` either way, but the reason is real and the
    /// lattice round must not have to guess which of the two it is.
    pub(crate) fn from_stream(document: &Document, stream: &Stream) -> Result<Self> {
        let dict = stream.dict();
        let bbox = dict
            .get(b"BBox")
            .and_then(rect_array)
            .ok_or_else(|| Error::malformed("a tiling pattern without a /BBox"))?;
        let x_step = dict
            .get(b"XStep")
            .and_then(Object::as_real)
            .ok_or_else(|| Error::malformed("a tiling pattern without an /XStep"))?;
        let y_step = dict
            .get(b"YStep")
            .and_then(Object::as_real)
            .ok_or_else(|| Error::malformed("a tiling pattern without a /YStep"))?;
        if x_step == 0.0 || y_step == 0.0 {
            return Err(Error::malformed("a tiling pattern with a zero step"));
        }
        let paint_type = match dict.get(b"PaintType").and_then(Object::as_integer) {
            Some(2) => PaintType::Uncoloured,
            Some(1) | None => PaintType::Coloured,
            Some(_) => {
                return Err(Error::malformed(
                    "a tiling pattern with an unknown /PaintType",
                ));
            }
        };
        let tiling_type = dict
            .get(b"TilingType")
            .and_then(Object::as_integer)
            .unwrap_or(1);
        let matrix = dict
            .get(b"Matrix")
            .and_then(matrix_array)
            .unwrap_or(Matrix::IDENTITY);
        let resources = match dict.get(b"Resources") {
            Some(Object::Dictionary(resources)) => Some(resources.clone()),
            _ => None,
        };
        let content = document.decode_stream(stream)?;
        Ok(Self {
            bbox,
            matrix,
            x_step,
            y_step,
            paint_type,
            tiling_type,
            resources,
            content,
        })
    }
}

/// Four numbers from an array (`/BBox`).
fn rect_array(object: &Object) -> Option<[f64; 4]> {
    let items = object.as_array()?;
    if items.len() < 4 {
        return None;
    }
    let value = |index: usize| items.get(index).and_then(Object::as_real);
    Some([value(0)?, value(1)?, value(2)?, value(3)?])
}

/// Six numbers from an array (`/Matrix`).
fn matrix_array(object: &Object) -> Option<Matrix> {
    let items = object.as_array()?;
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

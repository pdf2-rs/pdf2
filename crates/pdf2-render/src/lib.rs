//! A rasteriser for the `pdf2` crate: paths, clips, transforms, dashes and the
//! beginnings of a content-stream interpreter.
//!
//! The walker reuses `pdf2`'s tokenizer and document model — there is no second
//! PDF parser here. What it can draw today:
//!
//! - path construction (`m`, `l`, `c`, `v`, `y`, `h`, `re`) in user space, with
//!   the CTM applied at flattening time;
//! - fills with the non-zero (`f`, `F`, `B`) and even-odd (`f*`, `B*`) rules;
//! - strokes with butt, round and square caps, round joins, and dash patterns
//!   (`d`) with a phase;
//! - clipping (`W`, `W*`) as a per-pixel mask that nested `q`/`Q` restores;
//! - colour operators `g`/`G`, `rg`/`RG` and the naive `k`/`K` conversion.
//!
//! Not yet drawn, and reported per page in [`Report::unsupported`] instead of
//! failing: text and images. Rendering is deliberately without anti-aliasing
//! for now: coverage is decided at pixel centres, which keeps the golden images
//! exact and the scanline code small.

mod canvas;
mod font;
mod function;
mod geom;
mod image;
mod path;
mod png;
mod raster;
mod shading;
mod type1;
mod walk;

pub use canvas::{Canvas, ClipMask, Color};
pub use font::{Font, Glyph};
pub use geom::{Matrix, Point};
pub use image::Image;
pub use path::{Cap, Dash, Join, Path, Segment, Subpath};
pub use pdf2::{Error, Rect, Result};
pub use png::encode_png;
pub use raster::{clip_mask, fill_polygons};
pub use walk::{render_page, render_page_number, RenderOptions, RenderedPage, Report};

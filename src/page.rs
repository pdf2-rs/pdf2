//! The document model: the page tree (§7.7.3) and XMP metadata (§14.3.2).
//!
//! The page tree is walked from the catalog's `/Pages` node. Attributes that
//! the standard marks as inheritable — `/MediaBox`, `/CropBox`, `/Rotate` and
//! `/Resources` — are carried down the walk and overridden where a node or page
//! states its own value, which is what makes "per-page boxes" correct for files
//! that state them once at the root.
//!
//! `/Count` is deliberately not trusted: it is a hint, and a wrong hint would
//! silently hide pages. Pages are counted by walking the tree (§7.7.3.2 says a
//! conforming reader shall not rely on `/Count`).

use alloc::vec;
use alloc::vec::Vec;

use alloc::collections::BTreeSet;

use crate::document::Document;
use crate::error::{Error, Result};
use crate::object::{Dictionary, Object, ObjectId};

/// Deepest page tree accepted. Big documents nest a handful of levels; a
/// hostile file must produce an error rather than a stack overflow.
pub const MAX_PAGE_TREE_DEPTH: usize = 64;

/// A rectangle in PDF user space: `[x0 y0 x1 y1]`.
///
/// Coordinates are normalised so that `x0 <= x1` and `y0 <= y1`, because
/// writers disagree about the order of the corners.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x0: f64,
    /// Bottom edge.
    pub y0: f64,
    /// Right edge.
    pub x1: f64,
    /// Top edge.
    pub y1: f64,
}

impl Rect {
    /// A rectangle from its four coordinates.
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self {
            x0: if x0 <= x1 { x0 } else { x1 },
            y0: if y0 <= y1 { y0 } else { y1 },
            x1: if x1 >= x0 { x1 } else { x0 },
            y1: if y1 >= y0 { y1 } else { y0 },
        }
    }

    /// Width in user-space units.
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    /// Height in user-space units.
    pub fn height(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Read a box from a four-element array of numbers.
    pub fn from_array(items: &[Object]) -> Result<Self> {
        if items.len() != 4 {
            return Err(Error::Malformed {
                what: "a page box must be an array of four numbers",
                offset: None,
            });
        }
        let mut values = [0.0_f64; 4];
        for (slot, item) in values.iter_mut().zip(items) {
            *slot = match item {
                Object::Integer(value) => *value as f64,
                Object::Real(value) => *value,
                _ => {
                    return Err(Error::Malformed {
                        what: "a page box must be an array of four numbers",
                        offset: None,
                    })
                }
            };
        }
        Ok(Self::new(values[0], values[1], values[2], values[3]))
    }
}

/// One page, with every inheritable attribute already resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    /// The page object's id, absent when the page dictionary sits directly in
    /// its parent's `/Kids` array.
    pub id: Option<ObjectId>,
    /// Effective `/MediaBox`, required by the standard for every page.
    pub media_box: Option<Rect>,
    /// Effective `/CropBox`, which defaults to the media box.
    pub crop_box: Option<Rect>,
    /// Effective `/Rotate`, normalised to 0, 90, 180 or 270.
    pub rotate: i64,
    /// Effective `/Resources`.
    pub resources: Option<Dictionary>,
    /// Content stream ids, in the order they are painted.
    pub contents: Vec<ObjectId>,
}

impl Page {
    /// Whether this page's media box contains the point `(x, y)`.
    pub fn contains(&self, x: f64, y: f64) -> bool {
        match self.media_box {
            Some(rect) => x >= rect.x0 && x <= rect.x1 && y >= rect.y0 && y <= rect.y1,
            None => false,
        }
    }
}

/// Inheritable attributes carried down the page tree.
#[derive(Debug, Clone, Default)]
struct Inherited {
    media_box: Option<Rect>,
    crop_box: Option<Rect>,
    rotate: i64,
    resources: Option<Dictionary>,
}

impl Document {
    /// The catalog dictionary.
    pub fn catalog_dict(&self) -> Result<Dictionary> {
        match self.catalog()? {
            Object::Dictionary(dict) => Ok(dict),
            _ => Err(Error::Malformed {
                what: "catalog /Root is not a dictionary",
                offset: None,
            }),
        }
    }

    /// Walk the page tree and return every page in document order.
    pub fn pages(&self) -> Result<Vec<Page>> {
        let catalog = self.catalog_dict()?;
        let root = catalog.get(b"Pages").ok_or(Error::Malformed {
            what: "catalog without /Pages",
            offset: None,
        })?;
        let mut pages = Vec::new();
        let mut visited = BTreeSet::new();
        self.collect_pages(root, &Inherited::default(), 0, &mut visited, &mut pages)?;
        Ok(pages)
    }

    /// Number of pages, counted by walking the tree rather than trusting
    /// `/Count`.
    pub fn page_count(&self) -> Result<usize> {
        Ok(self.pages()?.len())
    }

    /// The page at `index`, counting from zero.
    pub fn page(&self, index: usize) -> Result<Page> {
        self.pages()?
            .into_iter()
            .nth(index)
            .ok_or(Error::Malformed {
                what: "page index is past the last page",
                offset: None,
            })
    }

    /// The page's effective `/UserUnit`, defaulting to `1.0` (7.7.3.4).
    ///
    /// The attribute is inheritable, so the page dictionary's `/Parent` chain is
    /// walked; a missing entry, a cycle (bounded at 32 levels) or a
    /// non-positive value stops the walk at `1.0`.
    pub fn page_user_unit(&self, page: &Page) -> Result<f64> {
        let mut current = page.id;
        for _ in 0..32 {
            let Some(id) = current else {
                break;
            };
            let Object::Dictionary(dictionary) = self.get_object(id)? else {
                break;
            };
            if let Some(value) = dictionary.get(b"UserUnit") {
                let value = self.resolve(Some(value))?.and_then(|value| value.as_real());
                return Ok(match value {
                    Some(value) if value.is_finite() && value > 0.0 => value,
                    _ => 1.0,
                });
            }
            current = match dictionary.get(b"Parent") {
                Some(Object::Reference(parent)) => Some(*parent),
                _ => None,
            };
        }
        Ok(1.0)
    }

    /// The decoded content of a page, with its streams joined by a newline.
    ///
    /// This is the input the content-stream interpreter takes.
    pub fn page_content(&self, page: &Page) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for id in &page.contents {
            let stream = self.stream_data(*id)?;
            if !out.is_empty() {
                out.push(b'\n');
            }
            out.extend_from_slice(&stream);
        }
        Ok(out)
    }

    /// Whether the catalog carries an XMP `/Metadata` stream.
    pub fn has_metadata(&self) -> Result<bool> {
        Ok(self.catalog_dict()?.contains_key(b"Metadata"))
    }

    /// The decoded XMP metadata packet, if the document has one.
    pub fn metadata(&self) -> Result<Option<Vec<u8>>> {
        let catalog = self.catalog_dict()?;
        let Some(metadata) = catalog.get(b"Metadata") else {
            return Ok(None);
        };
        match self.resolve(Some(metadata))? {
            Some(Object::Stream(stream)) => Ok(Some(self.decode_stream(&stream)?)),
            _ => Err(Error::Malformed {
                what: "/Metadata must be a stream",
                offset: None,
            }),
        }
    }

    fn collect_pages(
        &self,
        node: &Object,
        inherited: &Inherited,
        depth: usize,
        visited: &mut BTreeSet<ObjectId>,
        pages: &mut Vec<Page>,
    ) -> Result<()> {
        if depth > MAX_PAGE_TREE_DEPTH {
            return Err(Error::Malformed {
                what: "page tree is nested too deeply",
                offset: None,
            });
        }
        let (dict, id) = match node {
            Object::Reference(id) => {
                if !visited.insert(*id) {
                    return Err(Error::Malformed {
                        what: "page tree contains a cycle",
                        offset: None,
                    });
                }
                match self.get_object(*id)? {
                    Object::Dictionary(dict) => (dict, Some(*id)),
                    _ => {
                        return Err(Error::Malformed {
                            what: "page tree node is not a dictionary",
                            offset: None,
                        })
                    }
                }
            }
            Object::Dictionary(dict) => (dict.clone(), None),
            _ => {
                return Err(Error::Malformed {
                    what: "page tree node is not a dictionary or reference",
                    offset: None,
                })
            }
        };

        let inherited = Inherited {
            media_box: self.box_of(&dict, b"MediaBox")?.or(inherited.media_box),
            crop_box: self.box_of(&dict, b"CropBox")?.or(inherited.crop_box),
            rotate: match dict.get(b"Rotate") {
                Some(rotate) => normalise_rotate(rotate)?,
                None => inherited.rotate,
            },
            resources: match dict.get(b"Resources") {
                Some(resources) => Some(self.resources_of(resources)?),
                None => inherited.resources.clone(),
            },
        };

        if let Some(kids) = dict.get(b"Kids") {
            // Only the array itself may be indirect: the entries stay
            // references so that each page keeps its object id.
            let kids = match kids {
                Object::Array(items) => items.clone(),
                Object::Reference(id) => match self.get_object(*id)? {
                    Object::Array(items) => items,
                    _ => {
                        return Err(Error::Malformed {
                            what: "/Kids must be an array",
                            offset: None,
                        })
                    }
                },
                _ => {
                    return Err(Error::Malformed {
                        what: "/Kids must be an array",
                        offset: None,
                    })
                }
            };
            for kid in &kids {
                self.collect_pages(kid, &inherited, depth + 1, visited, pages)?;
            }
            return Ok(());
        }

        let is_page = matches!(
            dict.get(b"Type").and_then(Object::as_name),
            Some(name) if name == b"Page"
        ) || dict.contains_key(b"MediaBox")
            || inherited.media_box.is_some();
        if !is_page {
            return Err(Error::Malformed {
                what: "page tree node has neither /Kids nor /Type /Page",
                offset: None,
            });
        }
        let crop_box = inherited.crop_box.or(inherited.media_box);
        pages.push(Page {
            id,
            media_box: inherited.media_box,
            crop_box,
            rotate: inherited.rotate,
            resources: inherited.resources,
            contents: page_contents(&dict)?,
        });
        Ok(())
    }

    fn box_of(&self, dict: &Dictionary, key: &[u8]) -> Result<Option<Rect>> {
        let Some(value) = dict.get(key) else {
            return Ok(None);
        };
        let value = self.resolve(Some(value))?.unwrap_or(Object::Null);
        match value {
            Object::Array(items) => Rect::from_array(&items).map(Some),
            _ => Err(Error::Malformed {
                what: "a page box must be an array of four numbers",
                offset: None,
            }),
        }
    }

    fn resources_of(&self, resources: &Object) -> Result<Dictionary> {
        match self.resolve(Some(resources))?.unwrap_or(Object::Null) {
            Object::Dictionary(dict) => Ok(dict),
            _ => Err(Error::Malformed {
                what: "/Resources must be a dictionary",
                offset: None,
            }),
        }
    }
}

/// `/Rotate` is a multiple of 90; normalise it into 0, 90, 180 or 270.
fn normalise_rotate(value: &Object) -> Result<i64> {
    let value = match value {
        Object::Integer(value) => *value,
        _ => {
            return Err(Error::Malformed {
                what: "/Rotate must be an integer",
                offset: None,
            })
        }
    };
    if value % 90 != 0 {
        return Err(Error::Malformed {
            what: "/Rotate must be a multiple of 90",
            offset: None,
        });
    }
    Ok(((value % 360) + 360) % 360)
}

/// Content stream ids of a page: one reference or an array of references.
fn page_contents(dict: &Dictionary) -> Result<Vec<ObjectId>> {
    match dict.get(b"Contents") {
        None | Some(Object::Null) => Ok(Vec::new()),
        Some(Object::Reference(id)) => Ok(vec![*id]),
        Some(Object::Array(items)) => {
            let mut contents = Vec::with_capacity(items.len());
            for item in items {
                match item {
                    Object::Reference(id) => contents.push(*id),
                    _ => {
                        return Err(Error::Malformed {
                            what: "/Contents array entries must be stream references",
                            offset: None,
                        })
                    }
                }
            }
            Ok(contents)
        }
        Some(_) => Err(Error::Malformed {
            what: "/Contents must be a stream reference or an array",
            offset: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalise_rotate, page_contents, Rect};
    use crate::object::{Dictionary, Object, ObjectId};

    #[test]
    fn rectangles_normalise_their_corners() {
        let rect = Rect::new(10.0, 20.0, 0.0, 0.0);
        assert_eq!(
            rect,
            Rect {
                x0: 0.0,
                y0: 0.0,
                x1: 10.0,
                y1: 20.0
            }
        );
        assert_eq!(rect.width(), 10.0);
        assert_eq!(rect.height(), 20.0);
    }

    #[test]
    fn rectangles_read_integer_and_real_arrays() {
        let rect = Rect::from_array(&[
            Object::Integer(0),
            Object::Integer(0),
            Object::Real(612.5),
            Object::Integer(792),
        ])
        .expect("rectangle");
        assert_eq!(rect.x1, 612.5);
        assert_eq!(rect.y1, 792.0);
    }

    #[test]
    fn malformed_rectangles_are_errors() {
        assert!(Rect::from_array(&[Object::Integer(0), Object::Integer(0)]).is_err());
        assert!(Rect::from_array(&[
            Object::Integer(0),
            Object::Integer(0),
            Object::Integer(1),
            Object::name("top"),
        ])
        .is_err());
    }

    #[test]
    fn rotations_are_normalised() {
        assert_eq!(normalise_rotate(&Object::Integer(0)).expect("0"), 0);
        assert_eq!(normalise_rotate(&Object::Integer(90)).expect("90"), 90);
        assert_eq!(normalise_rotate(&Object::Integer(450)).expect("450"), 90);
        assert_eq!(normalise_rotate(&Object::Integer(-90)).expect("-90"), 270);
        assert_eq!(normalise_rotate(&Object::Integer(360)).expect("360"), 0);
        assert!(normalise_rotate(&Object::Integer(45)).is_err());
        assert!(normalise_rotate(&Object::Real(90.0)).is_err());
    }

    #[test]
    fn contents_accept_one_reference_or_an_array() {
        let mut dict = Dictionary::new();
        assert!(page_contents(&dict).expect("absent").is_empty());

        dict.insert("Contents", Object::reference(ObjectId::new(4, 0)));
        assert_eq!(
            page_contents(&dict).expect("single"),
            vec![ObjectId::new(4, 0)]
        );

        dict.insert(
            "Contents",
            Object::array([
                Object::reference(ObjectId::new(4, 0)),
                Object::reference(ObjectId::new(5, 0)),
            ]),
        );
        assert_eq!(
            page_contents(&dict).expect("array"),
            vec![ObjectId::new(4, 0), ObjectId::new(5, 0)]
        );

        dict.insert("Contents", Object::Integer(1));
        assert!(page_contents(&dict).is_err());
    }
}

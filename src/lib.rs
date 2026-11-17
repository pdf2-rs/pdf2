//! A PDF 2.0 (ISO 32000-2) reader and writer.
//!
//! Base objects live in [`object`], the tokenizer in [`lexer`], and the parser
//! in [`parser`]. Once the cross-reference reader lands, documents are opened
//! through `Document::open`.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

mod cmap;
pub mod content;
pub mod crypto;
pub mod document;
pub mod encoding;
pub mod error;
pub mod fax;
pub mod filter;
pub mod lexer;
pub mod object;
pub mod page;
pub mod parser;
pub mod writer;

pub use content::{PageHits, SearchHit, TextSpan};
pub use crypto::Encryption;
pub use document::{DEFAULT_OBJECT_LIMIT, DEFAULT_PAGE_LIMIT, Document};
pub use encoding::Encoding;
pub use error::{Error, Result};
pub use filter::{Filter, FilterSpec, Predictor};
pub use lexer::{Lexer, Token};
pub use object::{Dictionary, Object, ObjectId, PdfString, Stream, StringFormat};
pub use page::{Page, Rect};
pub use parser::{IndirectObject, Parser, parse_indirect_object, parse_object};
pub use writer::DocumentBuilder;

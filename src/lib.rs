//! A PDF 2.0 (ISO 32000-2) reader and writer.
//!
//! Base objects live in [`object`], the tokenizer in [`lexer`], and the parser
//! in [`parser`]. Once the cross-reference reader lands, documents are opened
//! through `Document::open`.
#![doc = include_str!("../README.md")]
#![forbid(unsafe_code)]
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod content;
pub mod crypto;
pub mod document;
pub mod error;
pub mod filter;
pub mod lexer;
pub mod object;
pub mod page;
pub mod parser;
pub mod writer;

pub use crypto::Encryption;
pub use document::Document;
pub use error::{Error, Result};
pub use filter::{Filter, FilterSpec, Predictor};
pub use lexer::{Lexer, Token};
pub use object::{Dictionary, Object, ObjectId, PdfString, Stream, StringFormat};
pub use page::{Page, Rect};
pub use parser::{parse_indirect_object, parse_object, IndirectObject, Parser};
pub use writer::DocumentBuilder;

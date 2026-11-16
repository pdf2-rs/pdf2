//! Errors returned by the `pdf2` crate.
//!
//! Every parsing path in this crate reports failure through [`Error`]; nothing
//! is expected to panic on malformed input, which is what M10 of the project
//! plan pins down with a corpus of truncated and mutated files.

use alloc::string::String;

use core::fmt;

/// Result type used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;

/// Every failure mode the crate reports.
///
/// The set grows as new layers (cross-reference tables, filters, content
/// streams, encryption) are added, which is why it is `#[non_exhaustive]`:
/// matching on it from outside the crate needs a catch-all arm.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Error {
    /// The input ended in the middle of a construct.
    ///
    /// `what` names the construct being read, for example `"number"`.
    UnexpectedEof {
        /// The construct being read, for example `"number"`.
        what: &'static str,
    },

    /// A byte that cannot start the expected construct was found.
    ///
    /// `offset` is the position of `byte` within the input being read.
    UnexpectedByte {
        /// The offending byte.
        byte: u8,
        /// Where it was found.
        offset: usize,
        /// The construct being read, for example `"name"`.
        what: &'static str,
    },

    /// A numeric token could not be parsed.
    InvalidNumber {
        /// The offending token, as it appeared in the input.
        text: String,
    },

    /// A decoded stream grew past the document's decoded-bytes budget.
    ///
    /// `what` names the stage that hit it (`"a Flate stream"`, `"the
    /// unfiltered stream"`) and `limit` is the budget in bytes.
    Limit {
        /// The filter stage that exceeded the budget.
        what: &'static str,
        /// The budget, in bytes.
        limit: usize,
    },

    /// The input is structurally wrong in a way particular to `what`.
    ///
    /// `offset` is where the problem was noticed, if it is known.
    Malformed {
        /// The construct that is wrong, for example `"xref table"`.
        what: &'static str,
        /// Where the problem was noticed, when that is known.
        offset: Option<usize>,
    },

    /// Reading or writing a file failed.
    Io {
        /// The operating system's message.
        message: String,
    },

    /// A construct this version does not implement yet.
    ///
    /// Unlike [`Error::Malformed`], the file is not wrong: the reader simply
    /// does not support the feature, and callers may choose to skip it.
    ///
    /// The variant is last on purpose: inserting one earlier would shift the
    /// implicit discriminants of every variant after it, which `cargo
    /// semver-checks` rightly treats as a breaking change for a public enum.
    Unsupported {
        /// What is missing, for example `"composite (Type0) fonts"`.
        what: &'static str,
    },
}

impl Error {
    /// The input ended while `what` was being read.
    pub const fn unexpected_eof(what: &'static str) -> Self {
        Self::UnexpectedEof { what }
    }

    /// The input is structurally wrong in a way particular to `what`.
    pub const fn malformed(what: &'static str) -> Self {
        Self::Malformed { what, offset: None }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof { what } => {
                write!(f, "unexpected end of input while reading {what}")
            }
            Self::UnexpectedByte { byte, offset, what } => write!(
                f,
                "unexpected byte {byte:#04X} at offset {offset} while reading {what}"
            ),
            Self::InvalidNumber { text } => write!(f, "invalid number {text:?}"),
            Self::Malformed {
                what,
                offset: Some(offset),
            } => write!(f, "malformed {what} at offset {offset}"),
            Self::Malformed { what, offset: None } => write!(f, "malformed {what}"),
            Self::Limit { what, limit } => write!(
                f,
                "{what} would decode past the {limit}-byte decoded-stream budget"
            ),
            Self::Unsupported { what } => write!(f, "unsupported: {what}"),
            Self::Io { message } => write!(f, "i/o error: {message}"),
        }
    }
}

impl core::error::Error for Error {}

#[cfg(feature = "std")]
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io {
            message: error.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn display_names_the_construct() {
        assert_eq!(
            Error::unexpected_eof("number").to_string(),
            "unexpected end of input while reading number"
        );
        assert_eq!(
            Error::Malformed {
                what: "xref table",
                offset: Some(12),
            }
            .to_string(),
            "malformed xref table at offset 12"
        );
        assert_eq!(Error::malformed("stream").to_string(), "malformed stream");
        assert_eq!(
            Error::Unsupported {
                what: "composite (Type0) fonts"
            }
            .to_string(),
            "unsupported: composite (Type0) fonts",
            "a valid construct this version cannot read is not called malformed"
        );
    }

    #[test]
    fn unexpected_byte_is_reported_in_hex() {
        assert_eq!(
            Error::UnexpectedByte {
                byte: 0x0A,
                offset: 3,
                what: "name",
            }
            .to_string(),
            "unexpected byte 0x0A at offset 3 while reading name"
        );
    }
}

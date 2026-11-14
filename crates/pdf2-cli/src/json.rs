//! The smallest JSON writer that produces valid output for this CLI.
//!
//! Output is hand-written rather than pulled from a crate: the command has no
//! dependencies, and strings from a PDF need careful escaping anyway.

/// A JSON string literal, with quotes, backslashes and control characters
/// escaped. Valid UTF-8 is passed through unchanged.
pub fn string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            character if (character as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => out.push(character),
        }
    }
    out.push('"');
    out
}

/// A JSON number, or `null` when absent.
pub fn number(value: Option<f64>) -> String {
    match value {
        Some(value) if value.is_finite() => format!("{value}"),
        _ => "null".to_owned(),
    }
}

/// A JSON array of numbers, or `null` when absent.
pub fn numbers(values: Option<[f64; 4]>) -> String {
    match values {
        Some(values) => format!(
            "[{}, {}, {}, {}]",
            number(Some(values[0])),
            number(Some(values[1])),
            number(Some(values[2])),
            number(Some(values[3]))
        ),
        None => "null".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::{number, numbers, string};

    #[test]
    fn strings_are_escaped() {
        assert_eq!(string("plain"), "\"plain\"");
        assert_eq!(string("a\"b"), "\"a\\\"b\"");
        assert_eq!(string("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(string("line\nbreak"), "\"line\\nbreak\"");
        assert_eq!(string("tab\there"), "\"tab\\there\"");
        assert_eq!(string("bell\u{07}"), "\"bell\\u0007\"");
        assert_eq!(string("caf\u{e9}"), "\"caf\u{e9}\"");
        assert_eq!(string(""), "\"\"");
    }

    #[test]
    fn numbers_keep_their_value_and_null_stays_null() {
        assert_eq!(number(Some(612.0)), "612");
        assert_eq!(number(Some(0.5)), "0.5");
        assert_eq!(number(None), "null");
        assert_eq!(numbers(Some([0.0, 0.0, 612.0, 792.0])), "[0, 0, 612, 792]");
        assert_eq!(numbers(None), "null");
    }
}

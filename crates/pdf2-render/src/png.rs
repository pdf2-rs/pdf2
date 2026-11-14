//! PNG encoding: RGBA8, filter 0, zlib through `miniz_oxide`.
//!
//! The goldens are compared byte-for-byte, so the encoder must be
//! deterministic — which a fixed filter choice, no ancillary chunks and a
//! fixed compression level give.

use crate::canvas::Canvas;

const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

/// Encode the canvas as a PNG.
pub fn encode_png(canvas: &Canvas) -> Vec<u8> {
    let width = canvas.width();
    let height = canvas.height();
    let mut raw = Vec::with_capacity((width as usize * 4 + 1) * height as usize);
    for row in 0..height {
        raw.push(0);
        let start = row as usize * width as usize * 4;
        let end = start + width as usize * 4;
        raw.extend_from_slice(&canvas.pixels()[start..end]);
    }
    let compressed = miniz_oxide::deflate::compress_to_vec_zlib(&raw, 6);

    let mut png = Vec::new();
    png.extend_from_slice(&SIGNATURE);
    let mut header = Vec::with_capacity(13);
    header.extend_from_slice(&width.to_be_bytes());
    header.extend_from_slice(&height.to_be_bytes());
    header.extend_from_slice(&[8, 6, 0, 0, 0]);
    write_chunk(&mut png, b"IHDR", &header);
    write_chunk(&mut png, b"IDAT", &compressed);
    write_chunk(&mut png, b"IEND", &[]);
    png
}

fn write_chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(kind);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xffff_ffff_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xedb8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::{crc32, encode_png};
    use crate::canvas::{Canvas, Color};

    #[test]
    fn crc32_matches_the_known_check_value() {
        // The standard check value: CRC-32 of "123456789".
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }

    #[test]
    fn a_png_has_the_expected_framing() {
        let canvas = Canvas::new(2, 1, Color::BLACK);
        let png = encode_png(&canvas);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        // IHDR length is always 13 and comes first.
        assert_eq!(&png[8..12], &13_u32.to_be_bytes());
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2_u32.to_be_bytes(), "width");
        assert_eq!(&png[20..24], &1_u32.to_be_bytes(), "height");
        assert!(png.ends_with(b"IEND\xaeB`\x82"), "trailing IEND chunk");
    }

    #[test]
    fn encoding_is_deterministic() {
        let canvas = Canvas::new(3, 3, Color::rgb(0.25, 0.5, 0.75));
        assert_eq!(encode_png(&canvas), encode_png(&canvas));
    }
}

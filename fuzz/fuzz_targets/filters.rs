//! Stream filters and predictors: the fuzz input is wrapped in a stream object
//! whose `/Filter` is chosen by the first byte, then decoded.
#![no_main]

use libfuzzer_sys::fuzz_target;

const FILTERS: [&str; 5] = [
    "FlateDecode",
    "LZWDecode",
    "ASCIIHexDecode",
    "ASCII85Decode",
    "RunLengthDecode",
];

fuzz_target!(|data: &[u8]| {
    let Some((&selector, payload)) = data.split_first() else {
        return;
    };
    let filter = FILTERS[usize::from(selector) % FILTERS.len()];
    let parms = if selector & 0x80 != 0 {
        format!(
            "/DecodeParms << /Predictor {} /Colors {} /BitsPerComponent 8 /Columns {} >>",
            2 + selector % 14,
            1 + selector % 4,
            1 + u16::from(selector),
        )
    } else {
        String::new()
    };
    let mut object = format!(
        "1 0 obj\n<< /Length {} /Filter /{filter} {parms} >>\nstream\n",
        payload.len()
    )
    .into_bytes();
    object.extend_from_slice(payload);
    object.extend_from_slice(b"\nendstream\nendobj");
    if let Ok(indirect) = pdf2::parse_indirect_object(&object) {
        if let Some(stream) = indirect.object.as_stream() {
            let _ = stream.decode();
        }
    }
});

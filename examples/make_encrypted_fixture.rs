//! Generate the AES-256 (revision 6) encryption fixtures.
//!
//! Run from the repository root:
//!
//! ```text
//! cargo run --example make_encrypted_fixture
//! ```
//!
//! It writes two committed fixtures with the same object graph:
//!
//! - `tests/fixtures/plain-twin.pdf` — unencrypted, `/Lang (en-US)` in the
//!   catalog and one content stream.
//! - `tests/fixtures/encrypted-r6.pdf` — the same document with `/V 5 /R 6`
//!   AES-256: the catalog string and the content stream are encrypted, and an
//!   `/Encrypt` dictionary carries `/U`, `/O`, `/UE`, `/OE` and `/Perms`.
//!
//! User password: `user`. Owner password: `owner`. No tool in this environment
//! can produce an R6 file (`qpdf`, `mutool`, `gs` are absent), so the fixtures
//! are generated here and `cargo test` reads them back; the salts, IVs and
//! "random" bytes are fixed, so re-running this program reproduces the files
//! byte for byte.

use std::io::Write;

use aes::cipher::{block_padding::NoPadding, BlockEncrypt, BlockEncryptMut, KeyInit, KeyIvInit};
use aes::{Aes128, Aes256};
use sha2::{Digest, Sha256, Sha384, Sha512};

const USER_PASSWORD: &[u8] = b"user";
const OWNER_PASSWORD: &[u8] = b"owner";
const USER_VALIDATION_SALT: &[u8; 8] = b"uvsalt01";
const USER_KEY_SALT: &[u8; 8] = b"uksalt01";
const OWNER_VALIDATION_SALT: &[u8; 8] = b"ovsalt01";
const OWNER_KEY_SALT: &[u8; 8] = b"oksalt01";
const CONTENT: &[u8] = b"BT /F1 24 Tf 72 700 Td (Encrypted hello) Tj ET";
const XMP: &[u8] = concat!(
    "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n",
    "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF ",
    "xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"/></x:xmpmeta>\n",
    "<?xpacket end=\"w\"?>"
)
.as_bytes();

fn hex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).expect("hex"), 16).expect("hex"))
        .collect()
}

/// Algorithm 2.B, written independently of the reader implementation.
fn hash_r6(password: &[u8], salt: &[u8], udata: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(password);
    hasher.update(salt);
    hasher.update(udata);
    let mut k = hasher.finalize().to_vec();
    let mut round = 0_u32;
    loop {
        let mut unit = password.to_vec();
        unit.extend_from_slice(&k);
        unit.extend_from_slice(udata);
        let mut k1 = Vec::new();
        for _ in 0..64 {
            k1.extend_from_slice(&unit);
        }
        let length = k1.len();
        let e = cbc::Encryptor::<Aes128>::new_from_slices(&k[..16], &k[16..32])
            .expect("aes128")
            .encrypt_padded_mut::<NoPadding>(&mut k1, length)
            .expect("encrypt")
            .to_vec();
        let sum: u32 = e[..16].iter().map(|byte| u32::from(*byte)).sum();
        k = match sum % 3 {
            0 => Sha256::digest(&e).to_vec(),
            1 => Sha384::digest(&e).to_vec(),
            _ => Sha512::digest(&e).to_vec(),
        };
        round += 1;
        if round >= 64 && u32::from(e[e.len() - 1]) <= round - 32 {
            break;
        }
    }
    let mut key = [0_u8; 32];
    key.copy_from_slice(&k[..32]);
    key
}

/// AES-256-CBC with PKCS#7 padding, prefixed by the IV, as PDF stores it.
fn encrypt_payload(key: &[u8; 32], iv: &[u8; 16], plaintext: &[u8]) -> Vec<u8> {
    let padding = 16 - plaintext.len() % 16;
    let mut buffer = plaintext.to_vec();
    buffer.extend(std::iter::repeat_n(padding as u8, padding));
    let length = buffer.len();
    let encrypted = cbc::Encryptor::<Aes256>::new_from_slices(key, iv)
        .expect("aes256")
        .encrypt_padded_mut::<NoPadding>(&mut buffer, length)
        .expect("encrypt")
        .to_vec();
    let mut out = iv.to_vec();
    out.extend_from_slice(&encrypted);
    out
}

/// AES-256-CBC without padding, zero IV, used for `/UE` and `/OE`.
fn encrypt_key(key: &[u8; 32], plaintext: &[u8; 32]) -> [u8; 32] {
    let mut buffer = *plaintext;
    let encrypted = cbc::Encryptor::<Aes256>::new_from_slices(key, &[0_u8; 16])
        .expect("aes256")
        .encrypt_padded_mut::<NoPadding>(&mut buffer, 32)
        .expect("encrypt");
    let mut out = [0_u8; 32];
    out.copy_from_slice(encrypted);
    out
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

/// Collects indirect objects and writes a classic cross-reference table.
struct Builder {
    out: Vec<u8>,
    offsets: Vec<(u32, usize)>,
}

impl Builder {
    fn new() -> Self {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-2.0\n");
        Self {
            out,
            offsets: Vec::new(),
        }
    }

    fn obj(&mut self, number: u32, body: &[u8]) {
        self.offsets.push((number, self.out.len()));
        self.out
            .extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        self.out.extend_from_slice(body);
        self.out.extend_from_slice(b"\nendobj\n");
    }

    fn stream(&mut self, number: u32, entries: &str, data: &[u8]) {
        self.offsets.push((number, self.out.len()));
        self.out
            .extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        self.out.extend_from_slice(
            format!("<< /Length {} {entries} >>\nstream\n", data.len()).as_bytes(),
        );
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    fn finish(mut self, trailer_extra: &str) -> Vec<u8> {
        let xref = self.out.len();
        let size = self.offsets.len() + 1;
        self.out
            .extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
        self.out.extend_from_slice(b"0000000000 65535 f \n");
        for (_, offset) in &self.offsets {
            self.out
                .extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        self.out.extend_from_slice(
            format!(
                "trailer\n<< /Size {size} /Root 1 0 R {trailer_extra} /ID \
                 [<00112233445566778899AABBCCDDEEFF><00112233445566778899AABBCCDDEEFF>] >>\n"
            )
            .as_bytes(),
        );
        self.out
            .extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
        self.out
    }
}

fn main() -> std::io::Result<()> {
    // --- keys and the encrypt dictionary -------------------------------
    let mut user = Vec::new();
    user.extend_from_slice(&hash_r6(USER_PASSWORD, USER_VALIDATION_SALT, &[]));
    user.extend_from_slice(USER_VALIDATION_SALT);
    user.extend_from_slice(USER_KEY_SALT);
    assert_eq!(user.len(), 48);

    let mut owner = Vec::new();
    owner.extend_from_slice(&hash_r6(OWNER_PASSWORD, OWNER_VALIDATION_SALT, &user));
    owner.extend_from_slice(OWNER_VALIDATION_SALT);
    owner.extend_from_slice(OWNER_KEY_SALT);

    let file_key = hash_r6(USER_PASSWORD, USER_KEY_SALT, &[]);
    let owner_key = hash_r6(OWNER_PASSWORD, OWNER_KEY_SALT, &user);
    let user_encrypted_key = encrypt_key(&file_key, &file_key);
    let owner_encrypted_key = encrypt_key(&owner_key, &file_key);

    // /Perms: P, 0xFFFFFFFF, 'T', "adb", 4 random bytes, AES-256-ECB.
    let mut perms = Vec::new();
    perms.extend_from_slice(&(-1_i32).to_le_bytes());
    perms.extend_from_slice(&[0xFF; 4]);
    perms.push(b'T');
    perms.extend_from_slice(b"adb");
    perms.extend_from_slice(&hex("deadbeef"));
    let mut perms_block = [0_u8; 16];
    perms_block.copy_from_slice(&perms);
    Aes256::new_from_slice(&file_key)
        .expect("key")
        .encrypt_block((&mut perms_block).into());
    let perms_encrypted = perms_block;

    let encrypt_dict = format!(
        "<< /Filter /Standard /V 5 /R 6 /Length 256 \
         /CF << /StdCF << /CFM /AESV3 /Length 32 >> >> /StmF /StdCF /StrF /StdCF \
         /U <{u}> /O <{o}> /UE <{ue}> /OE <{oe}> /Perms <{perms}> /P -1 /EncryptMetadata true >>",
        u = hex_string(&user),
        o = hex_string(&owner),
        ue = hex_string(&user_encrypted_key),
        oe = hex_string(&owner_encrypted_key),
        perms = hex_string(&perms_encrypted),
    );

    // --- the shared object graph ---------------------------------------
    let catalog =
        |lang: &str| format!("<< /Type /Catalog /Pages 2 0 R /Lang {lang} /Metadata 6 0 R >>");
    let pages = "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string();
    let page = "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                /Resources << /Font << /F1 5 0 R >> >> >>"
        .to_string();
    let font = "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
        .to_string();

    // --- plain twin -----------------------------------------------------
    let mut plain = Builder::new();
    plain.obj(1, catalog("(en-US)").as_bytes());
    plain.obj(2, pages.as_bytes());
    plain.obj(3, page.as_bytes());
    plain.stream(4, "", CONTENT);
    plain.obj(5, font.as_bytes());
    plain.stream(6, "/Type /Metadata /Subtype /XML", XMP);
    let plain = plain.finish("");

    // --- encrypted file --------------------------------------------------
    let lang = encrypt_payload(
        &file_key,
        &hex("000102030405060708090a0b0c0d0e0f")[..16]
            .try_into()
            .expect("iv"),
        b"en-US",
    );
    let content = encrypt_payload(
        &file_key,
        &hex("101112131415161718191a1b1c1d1e1f")[..16]
            .try_into()
            .expect("iv"),
        CONTENT,
    );
    let metadata = encrypt_payload(
        &file_key,
        &hex("202122232425262728292a2b2c2d2e2f")[..16]
            .try_into()
            .expect("iv"),
        XMP,
    );

    let mut encrypted = Builder::new();
    encrypted.obj(1, catalog(&format!("<{}>", hex_string(&lang))).as_bytes());
    encrypted.obj(2, pages.as_bytes());
    encrypted.obj(3, page.as_bytes());
    encrypted.stream(4, "", &content);
    encrypted.obj(5, font.as_bytes());
    encrypted.stream(6, "/Type /Metadata /Subtype /XML", &metadata);
    encrypted.obj(7, encrypt_dict.as_bytes());
    let encrypted = encrypted.finish("/Encrypt 7 0 R");

    let mut plain_file = std::fs::File::create("tests/fixtures/plain-twin.pdf")?;
    plain_file.write_all(&plain)?;
    let mut encrypted_file = std::fs::File::create("tests/fixtures/encrypted-r6.pdf")?;
    encrypted_file.write_all(&encrypted)?;
    println!(
        "plain-twin.pdf: {} bytes, encrypted-r6.pdf: {} bytes (user password \"user\")",
        plain.len(),
        encrypted.len()
    );
    Ok(())
}

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

// --- revisions 2–4 ------------------------------------------------------
//
// The legacy path derives a per-object key from the file key, the object number
// and its generation, so the helpers below take the object they encrypt. The
// MD5 here is a second implementation on purpose: the reader has its own, and a
// mistake shared by both would make the round-trip test agree with itself.

fn md5(data: &[u8]) -> [u8; 16] {
    const SHIFTS: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    #[allow(clippy::unreadable_literal)]
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut state = [0x67452301_u32, 0xefcdab89, 0x98badcfe, 0x10325476];
    let mut message = data.to_vec();
    let bits = (data.len() as u64) * 8;
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_le_bytes());
    for block in message.chunks_exact(64) {
        let mut words = [0_u32; 16];
        for (index, word) in words.iter_mut().enumerate() {
            let at = index * 4;
            *word = u32::from_le_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]]);
        }
        let [mut a, mut b, mut c, mut d] = state;
        for round in 0..64 {
            let (mixed, index) = match round / 16 {
                0 => ((b & c) | (!b & d), round),
                1 => ((d & b) | (!d & c), (round * 5 + 1) % 16),
                2 => (b ^ c ^ d, (round * 3 + 5) % 16),
                _ => (c ^ (b | !d), (round * 7) % 16),
            };
            let value = a
                .wrapping_add(mixed)
                .wrapping_add(K[round])
                .wrapping_add(words[index]);
            a = d;
            d = c;
            c = b;
            b = b.wrapping_add(value.rotate_left(SHIFTS[round]));
        }
        state[0] = state[0].wrapping_add(a);
        state[1] = state[1].wrapping_add(b);
        state[2] = state[2].wrapping_add(c);
        state[3] = state[3].wrapping_add(d);
    }
    let mut digest = [0_u8; 16];
    for (index, word) in state.iter().enumerate() {
        digest[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    digest
}

fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut state: Vec<u8> = (0..=255).collect();
    let mut j = 0_u8;
    for i in 0..256 {
        j = j.wrapping_add(state[i]).wrapping_add(key[i % key.len()]);
        state.swap(i, usize::from(j));
    }
    let (mut i, mut j) = (0_u8, 0_u8);
    data.iter()
        .map(|byte| {
            i = i.wrapping_add(1);
            j = j.wrapping_add(state[usize::from(i)]);
            state.swap(usize::from(i), usize::from(j));
            let index = state[usize::from(i)].wrapping_add(state[usize::from(j)]);
            byte ^ state[usize::from(index)]
        })
        .collect()
}

const PADDING: [u8; 32] = [
    0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01, 0x08,
    0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53, 0x69, 0x7A,
];

fn pad(password: &[u8]) -> [u8; 32] {
    let mut buffer = [0_u8; 32];
    let taken = password.len().min(32);
    buffer[..taken].copy_from_slice(&password[..taken]);
    buffer[taken..].copy_from_slice(&PADDING[..32 - taken]);
    buffer
}

/// Algorithm 2 with a 128-bit key, as revisions 3 and 4 use.
fn file_key_r4(password: &[u8], owner: &[u8], id0: &[u8]) -> Vec<u8> {
    let mut input = pad(password).to_vec();
    input.extend_from_slice(&owner[..32]);
    input.extend_from_slice(&(-3904_i32).to_le_bytes());
    input.extend_from_slice(id0);
    let mut digest = md5(&input);
    for _ in 0..50 {
        digest = md5(&digest);
    }
    digest.to_vec()
}

/// Algorithm 3: `/O` from the owner and user passwords.
fn owner_value(owner_password: &[u8], user_password: &[u8]) -> Vec<u8> {
    let mut digest = md5(&pad(owner_password));
    for _ in 0..50 {
        digest = md5(&digest);
    }
    let key = digest;
    let mut value = rc4(&key, &pad(user_password));
    for round in 1..=19u8 {
        let round_key: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
        value = rc4(&round_key, &value);
    }
    value
}

/// Algorithm 5: `/U` from the file key and the `/ID`.
fn user_value(key: &[u8], id0: &[u8]) -> Vec<u8> {
    let mut input = pad(b"").to_vec();
    input.extend_from_slice(id0);
    let mut value = md5(&input).to_vec();
    value = rc4(key, &value);
    for round in 1..=19u8 {
        let round_key: Vec<u8> = key.iter().map(|byte| byte ^ round).collect();
        value = rc4(&round_key, &value);
    }
    // `/U` is 32 bytes: the 16-byte check followed by arbitrary padding.
    value.extend_from_slice(&[0xAB; 16]);
    value
}

/// Algorithm 1: the per-object key revisions 2–4 use.
fn object_key(file_key: &[u8], number: u32, aes: bool) -> Vec<u8> {
    let mut input = file_key.to_vec();
    input.extend_from_slice(&number.to_le_bytes()[..3]);
    input.extend_from_slice(&[0, 0]);
    if aes {
        input.extend_from_slice(b"sAlT");
    }
    let digest = md5(&input);
    digest[..(file_key.len() + 5).min(16)].to_vec()
}

fn encrypt_legacy(key: &[u8], plaintext: &[u8], aes: bool, iv: &[u8; 16]) -> Vec<u8> {
    if !aes {
        return rc4(key, plaintext);
    }
    let padding = 16 - plaintext.len() % 16;
    let mut buffer = plaintext.to_vec();
    buffer.extend(std::iter::repeat_n(padding as u8, padding));
    let length = buffer.len();
    let encrypted = cbc::Encryptor::<Aes128>::new_from_slices(key, iv)
        .expect("aes128")
        .encrypt_padded_mut::<NoPadding>(&mut buffer, length)
        .expect("encrypt")
        .to_vec();
    let mut out = iv.to_vec();
    out.extend_from_slice(&encrypted);
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
        Self::with_header("%PDF-2.0")
    }

    fn with_header(header: &str) -> Self {
        let mut out = Vec::new();
        out.extend_from_slice(format!("{header}\n").as_bytes());
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

    // --- revisions 2–4: the same graph, a legacy handler ----------------
    let id0 = hex("00112233445566778899AABBCCDDEEFF");
    let owner_value_r4 = owner_value(OWNER_PASSWORD, USER_PASSWORD);
    let legacy_key = file_key_r4(USER_PASSWORD, &owner_value_r4, &id0);
    let user_value_r4 = user_value(&legacy_key, &id0);

    for (aes, cfm, file) in [
        (false, "V2", "encrypted-r4-rc4.pdf"),
        (true, "AESV2", "encrypted-r4-aes.pdf"),
    ] {
        let encrypt_dict = format!(
            "<< /Filter /Standard /V 4 /R 4 /Length 128 \
             /CF << /StdCF << /CFM /{cfm} /Length 16 >> >> /StmF /StdCF /StrF /StdCF \
             /U <{u}> /O <{o}> /P -3904 /EncryptMetadata true >>",
            u = hex_string(&user_value_r4),
            o = hex_string(&owner_value_r4),
        );
        let lang = encrypt_legacy(
            &object_key(&legacy_key, 1, aes),
            b"en-US",
            aes,
            &hex("000102030405060708090a0b0c0d0e0f")[..16]
                .try_into()
                .expect("iv"),
        );
        let content = encrypt_legacy(
            &object_key(&legacy_key, 4, aes),
            CONTENT,
            aes,
            &hex("101112131415161718191a1b1c1d1e1f")[..16]
                .try_into()
                .expect("iv"),
        );
        let metadata = encrypt_legacy(
            &object_key(&legacy_key, 6, aes),
            XMP,
            aes,
            &hex("202122232425262728292a2b2c2d2e2f")[..16]
                .try_into()
                .expect("iv"),
        );
        // A revision 4 handler in a 2.0 header is contradictory; these files say
        // what they are.
        let mut legacy = Builder::with_header("%PDF-1.7");
        legacy.obj(1, catalog(&format!("<{}>", hex_string(&lang))).as_bytes());
        legacy.obj(2, pages.as_bytes());
        legacy.obj(3, page.as_bytes());
        legacy.stream(4, "", &content);
        legacy.obj(5, font.as_bytes());
        legacy.stream(6, "/Type /Metadata /Subtype /XML", &metadata);
        legacy.obj(7, encrypt_dict.as_bytes());
        let legacy = legacy.finish("/Encrypt 7 0 R");
        let mut handle = std::fs::File::create(format!("tests/fixtures/{file}"))?;
        handle.write_all(&legacy)?;
        println!("{file}: {} bytes (user password \"user\")", legacy.len());
    }

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

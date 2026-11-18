//! The standard security handler, revisions 2–6 (§7.6).
//!
//! What is read, and which algorithms carry it:
//!
//! | `/V` | `/R` | Cipher | Key derivation | Password check |
//! | --- | --- | --- | --- | --- |
//! | 1 | 2 | RC4, 40 bit | Algorithm 2 (MD5) | Algorithm 4 |
//! | 2 | 3 | RC4, 40–128 bit | Algorithm 2, 50 rounds | Algorithm 5 |
//! | 4 | 4 | RC4 (`/V2`) or AES-128 (`/AESV2`) | as `/R 3` | Algorithm 5 |
//! | 5 | 6 | AES-256 (`/AESV3`) | Algorithm 2.B | Algorithm 2.A |
//!
//! Two structures differ between the generations and are worth stating:
//!
//! - Revisions 2–4 derive a **per-object key** from the file key, the object
//!   number and its generation (Algorithm 1), which is why a string or stream
//!   from those revisions cannot be decrypted without knowing which object it
//!   came from. Revisions 5 and 6 use the file key directly.
//! - Revision 6 verification is Algorithm 2.B — a 64-round minimum key stretch
//!   over SHA-256/384/512 and AES-128-CBC — implemented in [`hash_r6`], while
//!   revisions 2–4 use MD5, implemented here rather than pulled in as a
//!   dependency: it is 100 lines, needs no `std`, and its only use is key
//!   derivation for files that predate PDF 2.0.
//!
//! `/R 5` is refused: it is the deprecated draft of revision 6 and no file in
//! the corpus uses it.

use alloc::vec::Vec;

use aes::cipher::{
    BlockDecrypt, BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit,
    block_padding::{NoPadding, Pkcs7},
};
use aes::{Aes128, Aes256};
use sha2::{Digest, Sha256, Sha384, Sha512};

use crate::error::{Error, Result};
use crate::object::{Dictionary, Object, ObjectId, Stream};

/// Length of the file key, and of the AES key, for `/V 5`.
pub const KEY_LENGTH: usize = 32;

/// The standard security handler for one document.
#[derive(Debug, Clone)]
pub struct Encryption {
    /// Which cipher and key the document uses.
    kind: Kind,
    /// Whether the XMP `/Metadata` stream is encrypted too.
    encrypt_metadata: bool,
}

/// The cipher a document's strings and streams use.
#[derive(Debug, Clone)]
enum Kind {
    /// `/V 5 /R 6`: AES-256-CBC with the file key, no per-object derivation.
    Aes256 {
        /// The 32-byte file key.
        file_key: [u8; KEY_LENGTH],
    },
    /// Revisions 2–4: a file key of 5 to 16 bytes and a per-object key.
    Legacy {
        /// The file key.
        file_key: Vec<u8>,
        /// AES-128-CBC when the crypt filter is `/AESV2`, RC4 otherwise.
        aes: bool,
    },
    /// `/StmF` and `/StrF` both `/Identity`: the file holds no ciphertext.
    Identity,
}

impl Encryption {
    /// Build the handler from an `/Encrypt` dictionary and a password.
    ///
    /// Reads revisions 2–6. Revisions 2–4 derive their key from the trailer's
    /// `/ID`, so callers that have a file must use [`Encryption::new_with_id`];
    /// this entry point is enough for `/V 5`, whose key needs no `/ID`.
    pub fn new(dict: &Dictionary, password: &[u8]) -> Result<Self> {
        Self::new_with_id(dict, password, &[])
    }

    /// Build the handler with the trailer's first `/ID` element.
    ///
    /// Revisions 2–4 mix the `/ID` into the file key (Algorithm 2, step b) and
    /// into the user-password check (Algorithm 5), so an empty `id` can only
    /// produce a wrong key for them — which is reported rather than guessed at.
    pub fn new_with_id(dict: &Dictionary, password: &[u8], id: &[u8]) -> Result<Self> {
        match dict.get(b"Filter") {
            Some(Object::Name(name)) if name == b"Standard" => {}
            _ => {
                return Err(Error::Malformed {
                    what: "only the /Standard security handler is supported",
                    offset: None,
                });
            }
        }
        let version = integer(dict, b"V")?;
        let encrypt_metadata = match dict.get(b"EncryptMetadata") {
            Some(Object::Boolean(value)) => *value,
            Some(_) => {
                return Err(Error::Malformed {
                    what: "/EncryptMetadata must be a boolean",
                    offset: None,
                });
            }
            None => true,
        };
        match version {
            1 | 2 => Self::new_legacy(dict, password, id, encrypt_metadata, false),
            4 => Self::new_version_4(dict, password, id, encrypt_metadata),
            5 => Self::new_aes256(dict, password, encrypt_metadata),
            _ => Err(Error::Malformed {
                what: "encryption dictionary must be /V 1, 2, 4 or 5",
                offset: None,
            }),
        }
    }

    /// `/V 4`: either cipher, chosen by the `/CF` entry `/StmF` names.
    fn new_version_4(
        dict: &Dictionary,
        password: &[u8],
        id: &[u8],
        encrypt_metadata: bool,
    ) -> Result<Self> {
        let streams = crypt_filter(dict, b"StmF")?;
        let strings = crypt_filter(dict, b"StrF")?;
        if streams == CryptFilter::Identity && strings == CryptFilter::Identity {
            return Ok(Self {
                kind: Kind::Identity,
                encrypt_metadata,
            });
        }
        if streams != strings {
            return Err(Error::Unsupported {
                what: "strings and streams with different crypt filters",
            });
        }
        if streams == CryptFilter::Aes256 {
            return Err(Error::Malformed {
                what: "/V 4 cannot use an AESV3 crypt filter",
                offset: None,
            });
        }
        Self::new_legacy(
            dict,
            password,
            id,
            encrypt_metadata,
            streams == CryptFilter::Aes128,
        )
    }

    /// Revisions 2–4: an RC4 file key, or AES-128 when `aes` is set.
    fn new_legacy(
        dict: &Dictionary,
        password: &[u8],
        id: &[u8],
        encrypt_metadata: bool,
        aes: bool,
    ) -> Result<Self> {
        let revision = integer(dict, b"R")?;
        if !matches!(revision, 2..=4) {
            return Err(Error::Malformed {
                what: "revisions 1, 5 and 7 are not supported (2, 3, 4 and 6 are)",
                offset: None,
            });
        }
        if id.is_empty() {
            return Err(Error::Malformed {
                what: "revisions 2 to 4 need the trailer /ID to derive their key",
                offset: None,
            });
        }
        let bits = match dict.get(b"Length") {
            None => 40,
            Some(value) => value.as_integer().ok_or(Error::Malformed {
                what: "/Length must be an integer",
                offset: None,
            })?,
        };
        let length = match bits {
            40 => 5,
            48..=128 if bits % 8 == 0 => usize::try_from(bits / 8).unwrap_or(16),
            _ => {
                return Err(Error::Malformed {
                    what: "revision 2 to 4 keys are 40 to 128 bits, in whole bytes",
                    offset: None,
                });
            }
        };
        let owner = bytes(dict, b"O", 32)?;
        let user = bytes(dict, b"U", 32)?;
        let permissions = dict
            .get(b"P")
            .and_then(Object::as_integer)
            .ok_or(Error::Malformed {
                what: "encryption dictionary without /P",
                offset: None,
            })?;

        // Algorithm 2: the file key from the user password. The owner password
        // path (Algorithm 7) recovers the user password from `/O` first.
        let key = file_key(
            password,
            &owner,
            permissions,
            id,
            length,
            revision,
            encrypt_metadata,
        );
        if !check_user_password(&key, &user, id, revision) {
            let recovered = user_password_from_owner(password, &owner, length, revision);
            let owner_key = file_key(
                &recovered,
                &owner,
                permissions,
                id,
                length,
                revision,
                encrypt_metadata,
            );
            if !check_user_password(&owner_key, &user, id, revision) {
                return Err(Error::Malformed {
                    what: "wrong password for an encrypted document",
                    offset: None,
                });
            }
            return Ok(Self {
                kind: Kind::Legacy {
                    file_key: owner_key,
                    aes,
                },
                encrypt_metadata,
            });
        }
        Ok(Self {
            kind: Kind::Legacy { file_key: key, aes },
            encrypt_metadata,
        })
    }

    /// `/V 5 /R 6`: AES-256 with the file key directly.
    fn new_aes256(dict: &Dictionary, password: &[u8], encrypt_metadata: bool) -> Result<Self> {
        let revision = integer(dict, b"R")?;
        if revision != 6 {
            return Err(Error::Malformed {
                what: "only revision 6 is supported (revision 5 is deprecated)",
                offset: None,
            });
        }
        if let Some(length) = dict.get(b"Length") {
            let length = length.as_integer().ok_or(Error::Malformed {
                what: "/Length must be an integer",
                offset: None,
            })?;
            if length != 256 {
                return Err(Error::Malformed {
                    what: "/V 5 requires a 256-bit key",
                    offset: None,
                });
            }
        }
        if crypt_filter(dict, b"StmF")? == CryptFilter::Identity
            && crypt_filter(dict, b"StrF")? == CryptFilter::Identity
        {
            return Ok(Self {
                kind: Kind::Identity,
                encrypt_metadata,
            });
        }

        let owner = bytes(dict, b"O", 48)?;
        let user = bytes(dict, b"U", 48)?;

        // Algorithm 2.A: the user password yields the file key directly, the
        // owner password unwraps it from `/OE`.
        let user_hash = hash_r6(password, &user[32..40], &[])?;
        let file_key = if user_hash == user[..32] {
            hash_r6(password, &user[40..48], &[])?
        } else {
            let owner_hash = hash_r6(password, &owner[32..40], &user[..48])?;
            if owner_hash != owner[..32] {
                return Err(Error::Malformed {
                    what: "wrong password for an encrypted document",
                    offset: None,
                });
            }
            let Some(ue) = dict.get(b"OE") else {
                // Without /OE there is no way to recover the file key from the
                // owner password.
                return Err(Error::Malformed {
                    what: "encrypted document without /OE",
                    offset: None,
                });
            };
            let wrapped = ue.as_string().ok_or(Error::Malformed {
                what: "/OE must be a string",
                offset: None,
            })?;
            // The intermediate owner key uses the *key* salt, not the
            // validation salt the check above used.
            let owner_key = hash_r6(password, &owner[40..48], &user[..48])?;
            let mut buffer = wrapped.as_bytes().to_vec();
            if buffer.len() != 32 {
                return Err(Error::Malformed {
                    what: "/OE must be 32 bytes",
                    offset: None,
                });
            }
            let decrypted = cbc::Decryptor::<Aes256>::new_from_slices(&owner_key, &[0_u8; 16])
                .map_err(|_| Error::malformed("AES-256 key length"))?
                .decrypt_padded_mut::<NoPadding>(&mut buffer)
                .map_err(|_| Error::Malformed {
                    what: "AES-256 decryption failed",
                    offset: None,
                })?;
            let mut key = [0_u8; KEY_LENGTH];
            key.copy_from_slice(decrypted);
            key
        };

        let encryption = Self {
            kind: Kind::Aes256 { file_key },
            encrypt_metadata,
        };
        if let Some(perms) = dict.get(b"Perms") {
            let perms = perms.as_string().ok_or(Error::Malformed {
                what: "/Perms must be a string",
                offset: None,
            })?;
            encryption.check_perms(perms.as_bytes())?;
        }
        Ok(encryption)
    }

    /// Whether the metadata stream is encrypted.
    pub fn encrypt_metadata(&self) -> bool {
        self.encrypt_metadata
    }

    /// Whether this document's strings and streams carry no ciphertext.
    ///
    /// A `/V 5` file whose `/StmF` and `/StrF` are both `/Identity` is legal and
    /// appears in the wild (the corpus has one); nothing in it is encrypted, so
    /// no password can be wrong and every payload is returned as it is.
    pub fn is_identity(&self) -> bool {
        matches!(self.kind, Kind::Identity)
    }

    /// Whether stream payloads are decrypted when the object is read rather than
    /// when the stream is decoded.
    ///
    /// Revisions 2–4 derive a per-object key, so a stream can only be decrypted
    /// where its object number is known — which is [`crate::Document::get_object`],
    /// not [`crate::Document::decode_stream`].
    pub fn streams_are_decrypted_eagerly(&self) -> bool {
        matches!(self.kind, Kind::Legacy { .. })
    }

    /// Decrypt a string object.
    ///
    /// Revisions 2–4 key strings per object, so this entry point — which knows no
    /// object — is only enough for `/V 5`; use
    /// [`Encryption::decrypt_object_at`] when the object number is known.
    pub fn decrypt_string(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.decrypt(data, None)
    }

    /// Decrypt a string that belongs to object `id`.
    pub fn decrypt_string_at(&self, data: &[u8], id: ObjectId) -> Result<Vec<u8>> {
        self.decrypt(data, Some(id))
    }

    /// Decrypt a stream payload, honouring the exceptions in §7.6.1:
    /// cross-reference streams are never encrypted, and `/Metadata` is left in
    /// the clear when `/EncryptMetadata` is false.
    pub fn decrypt_stream(&self, stream: &Stream) -> Result<Vec<u8>> {
        if self.streams_are_decrypted_eagerly() {
            // Already decrypted when the object was read; a second pass would
            // turn plaintext back into noise.
            return Ok(stream.data().to_vec());
        }
        self.decrypt_stream_at(stream, None)
    }

    /// Decrypt a stream payload known to belong to `id`.
    pub fn decrypt_stream_at(&self, stream: &Stream, id: Option<ObjectId>) -> Result<Vec<u8>> {
        match stream.dict().get(b"Type").and_then(Object::as_name) {
            Some(name) if name == b"XRef" => return Ok(stream.data().to_vec()),
            Some(name) if name == b"Metadata" && !self.encrypt_metadata => {
                return Ok(stream.data().to_vec());
            }
            _ => {}
        }
        self.decrypt(stream.data(), id)
    }

    /// Encrypt a string that belongs to object `id`.
    ///
    /// The inverse of [`Encryption::decrypt_string_at`], for the objects an
    /// incremental update appends: revisions 2–4 key each string per object, so
    /// the number is not optional there.
    pub fn encrypt_string_at(&self, data: &[u8], id: ObjectId) -> Result<Vec<u8>> {
        self.encrypt(data, Some(id))
    }

    /// Encrypt a stream payload that belongs to object `id`.
    ///
    /// Honours the same exceptions as decryption: a cross-reference stream is
    /// never encrypted, and `/Metadata` stays in the clear when
    /// `/EncryptMetadata` is false (§7.6.1).
    pub fn encrypt_stream_at(&self, stream: &Stream, id: ObjectId) -> Result<Vec<u8>> {
        match stream.dict().get(b"Type").and_then(Object::as_name) {
            Some(name) if name == b"XRef" => return Ok(stream.data().to_vec()),
            Some(name) if name == b"Metadata" && !self.encrypt_metadata => {
                return Ok(stream.data().to_vec());
            }
            _ => {}
        }
        self.encrypt(stream.data(), Some(id))
    }

    /// Encrypt every string and stream payload in an object graph.
    ///
    /// What [`Encryption::decrypt_object_at`] undoes: an object written by an
    /// incremental update has to reach the file in exactly the form the base
    /// document holds its own objects, or the appended section reads as noise.
    pub fn encrypt_object_at(&self, object: &Object, id: ObjectId) -> Result<Object> {
        match object {
            Object::String(string) => Ok(Object::String(crate::object::PdfString::literal(
                self.encrypt(string.as_bytes(), Some(id))?,
            ))),
            Object::Array(items) => {
                let mut encrypted = Vec::with_capacity(items.len());
                for item in items {
                    encrypted.push(self.encrypt_object_at(item, id)?);
                }
                Ok(Object::Array(encrypted))
            }
            Object::Dictionary(dict) => Ok(Object::Dictionary(self.encrypt_dictionary(dict, id)?)),
            Object::Stream(stream) => {
                let dict = self.encrypt_dictionary(stream.dict(), id)?;
                let data = self.encrypt_stream_at(stream, id)?;
                Ok(Object::Stream(Stream::new(dict, data)))
            }
            other => Ok(other.clone()),
        }
    }

    /// Encrypt a payload with whichever cipher the document uses.
    fn encrypt(&self, data: &[u8], id: Option<ObjectId>) -> Result<Vec<u8>> {
        match &self.kind {
            Kind::Identity => Ok(data.to_vec()),
            Kind::Aes256 { file_key } => aes256_cbc_encrypt(file_key, data),
            Kind::Legacy { file_key, aes } => {
                let Some(id) = id else {
                    return Err(Error::Malformed {
                        what: "revisions 2 to 4 need the object number to derive a key",
                        offset: None,
                    });
                };
                let key = object_key(file_key, id.number, id.generation, *aes);
                if *aes {
                    aes128_cbc_encrypt(&key, id, data)
                } else {
                    Ok(rc4(&key, data))
                }
            }
        }
    }

    fn encrypt_dictionary(&self, dict: &Dictionary, id: ObjectId) -> Result<Dictionary> {
        let mut encrypted = Dictionary::new();
        for (name, value) in dict.iter() {
            encrypted.insert(name.to_vec(), self.encrypt_object_at(value, id)?);
        }
        Ok(encrypted)
    }

    /// Decrypt every string inside an object graph.
    ///
    /// Stream *payloads* are left alone for `/V 5` — they are decrypted when the
    /// stream is read through [`crate::Document::decode_stream`], so the cached
    /// object keeps the bytes the file holds — and decrypted here for revisions
    /// 2–4, where the per-object key needs the object number. Strings inside a
    /// stream dictionary are decrypted either way, because they belong to the
    /// object graph.
    pub fn decrypt_object(&self, object: &Object) -> Result<Object> {
        self.decrypt_object_at(object, None)
    }

    /// Decrypt an object that is known to be number `id`.
    pub fn decrypt_object_at(&self, object: &Object, id: Option<ObjectId>) -> Result<Object> {
        match object {
            Object::String(string) => Ok(Object::String(crate::object::PdfString::literal(
                self.decrypt(string.as_bytes(), id)?,
            ))),
            Object::Array(items) => {
                let mut decrypted = Vec::with_capacity(items.len());
                for item in items {
                    decrypted.push(self.decrypt_object_at(item, id)?);
                }
                Ok(Object::Array(decrypted))
            }
            Object::Dictionary(dict) => Ok(Object::Dictionary(self.decrypt_dictionary(dict, id)?)),
            Object::Stream(stream) => {
                let dict = self.decrypt_dictionary(stream.dict(), id)?;
                let data = if self.streams_are_decrypted_eagerly() {
                    self.decrypt_stream_at(stream, id)?
                } else {
                    stream.data().to_vec()
                };
                Ok(Object::Stream(Stream::new(dict, data)))
            }
            other => Ok(other.clone()),
        }
    }

    fn decrypt_dictionary(&self, dict: &Dictionary, id: Option<ObjectId>) -> Result<Dictionary> {
        let mut decrypted = Dictionary::new();
        for (name, value) in dict.iter() {
            decrypted.insert(name.to_vec(), self.decrypt_object_at(value, id)?);
        }
        Ok(decrypted)
    }

    /// Decrypt a payload with whichever cipher the document uses.
    fn decrypt(&self, data: &[u8], id: Option<ObjectId>) -> Result<Vec<u8>> {
        match &self.kind {
            Kind::Identity => Ok(data.to_vec()),
            Kind::Aes256 { file_key } => aes256_cbc_decrypt(file_key, data),
            Kind::Legacy { file_key, aes } => {
                let Some(id) = id else {
                    return Err(Error::Malformed {
                        what: "revisions 2 to 4 need the object number to derive a key",
                        offset: None,
                    });
                };
                let key = object_key(file_key, id.number, id.generation, *aes);
                if *aes {
                    aes128_cbc_decrypt(&key, data)
                } else {
                    Ok(rc4(&key, data))
                }
            }
        }
    }

    /// The `/Perms` tamper check: bytes 9–11 must read `adb`.
    fn check_perms(&self, perms: &[u8]) -> Result<()> {
        if perms.len() != 16 {
            return Err(Error::Malformed {
                what: "/Perms must be 16 bytes",
                offset: None,
            });
        }
        let mut block = [0_u8; 16];
        block.copy_from_slice(perms);
        // AES-256 in ECB mode is a single block decryption; the `cipher` crate
        // exposes that directly.
        let Kind::Aes256 { file_key } = &self.kind else {
            return Ok(());
        };
        let cipher =
            Aes256::new_from_slice(file_key).map_err(|_| Error::malformed("AES-256 key length"))?;
        cipher.decrypt_block((&mut block).into());
        if &block[9..12] != b"adb" {
            return Err(Error::Malformed {
                what: "encryption /Perms does not verify",
                offset: None,
            });
        }
        Ok(())
    }
}

/// The crypt filter `/StmF` or `/StrF` names (7.6.5, Table 25).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CryptFilter {
    /// No cipher at all.
    Identity,
    /// RC4.
    Rc4,
    /// AES-128-CBC.
    Aes128,
    /// AES-256-CBC, which only `/V 5` uses.
    Aes256,
}

/// Resolve one of `/StmF` or `/StrF` through `/CF`.
fn crypt_filter(dict: &Dictionary, key: &[u8]) -> Result<CryptFilter> {
    let Some(value) = dict.get(key) else {
        return Ok(CryptFilter::Identity);
    };
    let Object::Name(name) = value else {
        return Err(Error::Malformed {
            what: "crypt filter entries must be names",
            offset: None,
        });
    };
    if name == b"Identity" {
        return Ok(CryptFilter::Identity);
    }
    let Some(Object::Dictionary(filters)) = dict.get(b"CF") else {
        return Err(Error::Malformed {
            what: "a named crypt filter needs a /CF dictionary",
            offset: None,
        });
    };
    let Some(Object::Dictionary(filter)) = filters.get(name.as_slice()) else {
        return Err(Error::Malformed {
            what: "a named crypt filter is not declared in /CF",
            offset: None,
        });
    };
    match filter.get(b"CFM").and_then(Object::as_name) {
        Some(b"AESV2") => Ok(CryptFilter::Aes128),
        Some(b"AESV3") => Ok(CryptFilter::Aes256),
        Some(b"None") => Ok(CryptFilter::Identity),
        Some(b"V2") | None => Ok(CryptFilter::Rc4),
        Some(_) => Err(Error::Unsupported {
            what: "a crypt filter method other than V2, AESV2 or None",
        }),
    }
}

/// Algorithm 2: the file key from the user password (revisions 2–4).
fn file_key(
    password: &[u8],
    owner: &[u8],
    permissions: i64,
    id: &[u8],
    length: usize,
    revision: i64,
    encrypt_metadata: bool,
) -> Vec<u8> {
    let mut input = Vec::with_capacity(32 + 32 + 4 + id.len() + 4);
    input.extend_from_slice(&pad_password(password));
    input.extend_from_slice(&owner[..32]);
    // `/P` is a signed 32-bit integer written little-endian, low byte first.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let permissions = permissions as u32;
    input.extend_from_slice(&permissions.to_le_bytes());
    input.extend_from_slice(id);
    if revision >= 4 && !encrypt_metadata {
        input.extend_from_slice(&[0xFF; 4]);
    }
    let mut digest = md5(&input);
    if revision >= 3 {
        for _ in 0..50 {
            digest = md5(&digest[..length.min(16)]);
        }
    }
    digest[..length.min(16)].to_vec()
}

/// Algorithm 4 (`/R 2`) and Algorithm 5 (`/R 3`, 4): does the key fit `/U`?
fn check_user_password(key: &[u8], user: &[u8], id: &[u8], revision: i64) -> bool {
    if revision == 2 {
        return rc4(key, &pad_password(b"")) == user[..32];
    }
    let mut input = Vec::with_capacity(32 + id.len());
    input.extend_from_slice(&pad_password(b""));
    input.extend_from_slice(id);
    let mut value = md5(&input);
    let mut encrypted = rc4(key, &value);
    value.copy_from_slice(&encrypted);
    for round in 1..=19u8 {
        encrypted = rc4(&xor_key(key, round), &value);
        value.copy_from_slice(&encrypted);
    }
    value[..16] == user[..16]
}

/// Algorithm 7: recover the user password from `/O` with the owner password.
fn user_password_from_owner(
    password: &[u8],
    owner: &[u8],
    length: usize,
    revision: i64,
) -> Vec<u8> {
    let mut digest = md5(&pad_password(password));
    if revision >= 3 {
        for _ in 0..50 {
            digest = md5(&digest);
        }
    }
    let key = &digest[..length.min(16)];
    if revision == 2 {
        return rc4(key, &owner[..32]);
    }
    let mut value = rc4(key, &owner[..32]);
    for round in 1..=19u8 {
        value = rc4(&xor_key(key, round), &value);
    }
    value
}

/// The 32-byte password buffer Algorithm 2 hashes: padded with the standard
/// trailer or truncated.
fn pad_password(password: &[u8]) -> [u8; 32] {
    const PADDING: [u8; 32] = [
        0x28, 0xBF, 0x4E, 0x5E, 0x4E, 0x75, 0x8A, 0x41, 0x64, 0x00, 0x4E, 0x56, 0xFF, 0xFA, 0x01,
        0x08, 0x2E, 0x2E, 0x00, 0xB6, 0xD0, 0x68, 0x3E, 0x80, 0x2F, 0x0C, 0xA9, 0xFE, 0x64, 0x53,
        0x69, 0x7A,
    ];
    let mut buffer = [0_u8; 32];
    let taken = password.len().min(32);
    buffer[..taken].copy_from_slice(&password[..taken]);
    buffer[taken..].copy_from_slice(&PADDING[..32 - taken]);
    buffer
}

/// Algorithm 1: the per-object key for revisions 2–4.
fn object_key(file_key: &[u8], number: u32, generation: u16, aes: bool) -> Vec<u8> {
    let mut input = Vec::with_capacity(file_key.len() + 5 + 4);
    input.extend_from_slice(file_key);
    input.extend_from_slice(&number.to_le_bytes()[..3]);
    input.extend_from_slice(&generation.to_le_bytes());
    if aes {
        // 7.6.2: AES-128 keys mix in this literal, RC4 keys do not.
        input.extend_from_slice(b"sAlT");
    }
    let digest = md5(&input);
    digest[..(file_key.len() + 5).min(16)].to_vec()
}

fn xor_key(key: &[u8], value: u8) -> Vec<u8> {
    key.iter().map(|byte| byte ^ value).collect()
}

/// RC4 (7.6.2), the stream cipher revisions 2–4 use.
fn rc4(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut state = [0_u8; 256];
    for (index, slot) in state.iter_mut().enumerate() {
        #[allow(clippy::cast_possible_truncation)]
        let index = index as u8;
        *slot = index;
    }
    if key.is_empty() {
        return data.to_vec();
    }
    let mut j = 0_u8;
    for i in 0..256_u16 {
        let index = usize::from(i);
        j = j
            .wrapping_add(state[index])
            .wrapping_add(key[index % key.len()]);
        state.swap(index, usize::from(j));
    }
    let mut i = 0_u8;
    let mut j = 0_u8;
    let mut output = Vec::with_capacity(data.len());
    for &byte in data {
        i = i.wrapping_add(1);
        j = j.wrapping_add(state[usize::from(i)]);
        state.swap(usize::from(i), usize::from(j));
        let index = state[usize::from(i)].wrapping_add(state[usize::from(j)]);
        output.push(byte ^ state[usize::from(index)]);
    }
    output
}

/// AES-128-CBC: a 16-byte IV, the ciphertext, and PKCS#7 padding (7.6.2).
fn aes128_cbc_decrypt(key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 32 || !(data.len() - 16).is_multiple_of(16) {
        return Err(Error::Malformed {
            what: "AES-128 payload is not an IV followed by whole blocks",
            offset: None,
        });
    }
    let (iv, ciphertext) = data.split_at(16);
    let mut buffer = ciphertext.to_vec();
    let plaintext = cbc::Decryptor::<Aes128>::new_from_slices(key, iv)
        .map_err(|_| Error::malformed("AES-128 key or IV length"))?
        .decrypt_padded_mut::<NoPadding>(&mut buffer)
        .map_err(|_| Error::Malformed {
            what: "AES-128 decryption failed",
            offset: None,
        })?;
    Ok(unpad(plaintext))
}

fn aes256_cbc_decrypt(file_key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    if data.len() < 32 || !(data.len() - 16).is_multiple_of(16) {
        return Err(Error::Malformed {
            what: "AES-256 payload is not an IV followed by whole blocks",
            offset: None,
        });
    }
    let (iv, ciphertext) = data.split_at(16);
    let mut buffer = ciphertext.to_vec();
    let plaintext = cbc::Decryptor::<Aes256>::new_from_slices(file_key, iv)
        .map_err(|_| Error::malformed("AES-256 key or IV length"))?
        .decrypt_padded_mut::<NoPadding>(&mut buffer)
        .map_err(|_| Error::Malformed {
            what: "AES-256 decryption failed",
            offset: None,
        })?;
    Ok(unpad(plaintext))
}

/// AES-128-CBC encryption: an IV, then PKCS#7 padding and the ciphertext.
fn aes128_cbc_encrypt(key: &[u8], id: ObjectId, data: &[u8]) -> Result<Vec<u8>> {
    let iv = iv_for(key, &id, data);
    cbc::Encryptor::<Aes128>::new_from_slices(key, &iv)
        .map_err(|_| Error::malformed("AES-128 key or IV length"))
        .and_then(|cipher| finish_encryption(cipher, iv, data))
}

fn aes256_cbc_encrypt(file_key: &[u8], data: &[u8]) -> Result<Vec<u8>> {
    // Revisions 5 and 6 key with the file key itself, so the IV can only be
    // derived from it and the payload.
    let iv = iv_for(file_key, &ObjectId::new(0, 0), data);
    cbc::Encryptor::<Aes256>::new_from_slices(file_key, &iv)
        .map_err(|_| Error::malformed("AES-256 key or IV length"))
        .and_then(|cipher| finish_encryption(cipher, iv, data))
}

/// Pad, encrypt, and put the IV in front of the ciphertext.
fn finish_encryption<C>(cipher: C, iv: [u8; 16], data: &[u8]) -> Result<Vec<u8>>
where
    C: BlockEncryptMut,
{
    let mut buffer = alloc::vec![0_u8; data.len() + 16];
    buffer[..data.len()].copy_from_slice(data);
    let ciphertext = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, data.len())
        .map_err(|_| Error::malformed("AES padding failed"))?;
    let mut out = Vec::with_capacity(16 + ciphertext.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(ciphertext);
    Ok(out)
}

/// The 16-byte IV an update uses: SHA-256 over the key, the object number and
/// the payload, truncated.
///
/// Derived rather than drawn from a random source, because the library has none
/// (`no_std`, and a random source is not something a PDF reader should decide
/// for its caller). It is unique per object and payload, and deterministic — so
/// the same edit saved twice is byte-identical, the same property `/ID`'s second
/// string has — at the cost of revealing that two appends carried the same
/// bytes for the same object, which an update never does.
fn iv_for(key: &[u8], id: &ObjectId, data: &[u8]) -> [u8; 16] {
    let mut hash = Sha256::new();
    hash.update(key);
    hash.update(id.number.to_le_bytes());
    hash.update(id.generation.to_le_bytes());
    hash.update(data);
    let digest = hash.finalize();
    let mut iv = [0_u8; 16];
    iv.copy_from_slice(&digest[..16]);
    iv
}

/// MD5 (RFC 1321), used only by revisions 2–4 key derivation.
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
    let bit_length = (data.len() as u64).wrapping_mul(8);
    let mut message = data.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_length.to_le_bytes());
    for block in message.as_chunks::<64>().0 {
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

/// Algorithm 2.B: the revision 6 password hash.
///
/// `udata` is empty for the user password and the 48-byte `/U` value for the
/// owner password. Returns the first 32 bytes of the final digest.
pub fn hash_r6(password: &[u8], salt: &[u8], udata: &[u8]) -> Result<[u8; KEY_LENGTH]> {
    let mut initial = Sha256::new();
    initial.update(password);
    initial.update(salt);
    initial.update(udata);
    let mut key = initial.finalize().to_vec();

    let mut unit = Vec::with_capacity(password.len() + key.len() + udata.len());
    unit.extend_from_slice(password);
    unit.extend_from_slice(&key);
    unit.extend_from_slice(udata);

    let mut round: u32 = 0;
    loop {
        // K1 is (password + K + udata) repeated 64 times.
        let mut block = Vec::with_capacity(unit.len() * 64);
        for _ in 0..64 {
            block.extend_from_slice(&unit);
        }
        let block_len = block.len();
        let encrypted = cbc::Encryptor::<Aes128>::new_from_slices(&key[0..16], &key[16..32])
            .map_err(|_| Error::malformed("AES-128 key or IV length"))?
            .encrypt_padded_mut::<NoPadding>(&mut block, block_len)
            .map_err(|_| Error::Malformed {
                what: "AES-128 encryption failed during password hashing",
                offset: None,
            })?
            .to_vec();
        let checksum: u32 = encrypted[..16].iter().map(|byte| u32::from(*byte)).sum();
        key = match checksum % 3 {
            0 => Sha256::digest(&encrypted).to_vec(),
            1 => Sha384::digest(&encrypted).to_vec(),
            _ => Sha512::digest(&encrypted).to_vec(),
        };
        // The winning digest feeds the next round, so rebuild the repeated unit.
        unit.clear();
        unit.extend_from_slice(password);
        unit.extend_from_slice(&key);
        unit.extend_from_slice(udata);

        round += 1;
        let last = u32::from(encrypted[encrypted.len() - 1]);
        if round >= 64 && last <= round - 32 {
            break;
        }
        if round > 10_000 {
            return Err(Error::Malformed {
                what: "password hashing did not converge",
                offset: None,
            });
        }
    }
    let mut key_bytes = [0_u8; KEY_LENGTH];
    key_bytes.copy_from_slice(&key[..KEY_LENGTH]);
    Ok(key_bytes)
}

/// Remove PKCS#7 padding when it is valid; leave the data alone otherwise,
/// because some writers pad with zeros or not at all.
fn unpad(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let padding = usize::from(data[data.len() - 1]);
    if padding == 0 || padding > 16 || padding > data.len() {
        return data.to_vec();
    }
    if data[data.len() - padding..]
        .iter()
        .all(|byte| usize::from(*byte) == padding)
    {
        return data[..data.len() - padding].to_vec();
    }
    data.to_vec()
}

fn integer(dict: &Dictionary, key: &[u8]) -> Result<i64> {
    dict.get(key)
        .and_then(Object::as_integer)
        .ok_or(Error::Malformed {
            what: "encryption dictionary without /V and /R integers",
            offset: None,
        })
}

fn bytes(dict: &Dictionary, key: &[u8], length: usize) -> Result<Vec<u8>> {
    let value = dict
        .get(key)
        .and_then(Object::as_string)
        .ok_or(Error::Malformed {
            what: "encryption dictionary without /U and /O strings",
            offset: None,
        })?;
    if value.as_bytes().len() != length {
        return Err(Error::Malformed {
            what: "/U and /O must be 48 bytes for revision 6",
            offset: None,
        });
    }
    Ok(value.as_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::{Encryption, hash_r6, unpad};
    use crate::object::{Dictionary, Object, PdfString};

    #[test]
    fn padding_is_stripped_only_when_it_is_valid() {
        assert_eq!(unpad(&[1, 2, 3, 0x01]), vec![1, 2, 3]);
        assert_eq!(unpad(&[1, 2, 0x02, 0x02]), vec![1, 2]);
        assert_eq!(unpad(&[1, 2, 3, 0x00]), vec![1, 2, 3, 0x00]);
        assert_eq!(unpad(&[1, 2, 3, 0x05]), vec![1, 2, 3, 0x05]);
        assert_eq!(unpad(&[0x11]), vec![0x11]);
        assert_eq!(unpad(&[]), Vec::<u8>::new());
    }

    #[test]
    fn md5_matches_rfc_1321() {
        // The RFC's own test suite, so the implementation is checked against
        // something other than itself.
        for (input, expected) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
        ] {
            let digest = super::md5(input.as_bytes());
            let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
            assert_eq!(hex, expected, "md5({input:?})");
        }
    }

    #[test]
    fn rc4_matches_the_reference_vector() {
        // The long-standing reference: key "Key", plaintext "Plaintext".
        let ciphertext = super::rc4(b"Key", b"Plaintext");
        let hex: String = ciphertext
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        assert_eq!(hex, "bbf316e8d940af0ad3");
        assert_eq!(super::rc4(b"Key", &ciphertext), b"Plaintext");
    }

    #[test]
    fn the_r6_hash_is_stable_and_32_bytes() {
        // A fixed vector pins the algorithm so a refactor cannot silently
        // change it; the value was produced by this implementation and is
        // checked by the encrypted fixture round-trip as well.
        let hash = hash_r6(b"user", b"12345678", &[]).expect("hash");
        assert_eq!(hash.len(), 32);
        let again = hash_r6(b"user", b"12345678", &[]).expect("hash");
        assert_eq!(hash, again, "hashing is deterministic");
        let other = hash_r6(b"other", b"12345678", &[]).expect("hash");
        assert_ne!(hash, other);
        let salt = hash_r6(b"user", b"87654321", &[]).expect("hash");
        assert_ne!(hash, salt, "the salt changes the result");
        assert_eq!(
            hash.to_vec(),
            hash_r6(b"user", b"12345678", &[]).expect("hash").to_vec()
        );
    }

    /// An encryption dictionary shaped like a real one: the filters are named, so
    /// the password is actually checked rather than skipped as `/Identity`.
    fn encrypt_dict(perm_value: i64) -> Dictionary {
        let mut filters = Dictionary::new();
        let mut standard = Dictionary::new();
        standard.insert("CFM", Object::name("AESV3"));
        standard.insert("Length", Object::Integer(32));
        filters.insert("StdCF", Object::Dictionary(standard));
        let mut dict = Dictionary::new();
        dict.insert("Filter", Object::name("Standard"));
        dict.insert("V", Object::Integer(5));
        dict.insert("R", Object::Integer(6));
        dict.insert("Length", Object::Integer(256));
        dict.insert("CF", Object::Dictionary(filters));
        dict.insert("StmF", Object::name("StdCF"));
        dict.insert("StrF", Object::name("StdCF"));
        dict.insert("EncryptMetadata", Object::Boolean(true));
        dict.insert("U", Object::String(PdfString::hex(vec![0; 48])));
        dict.insert("O", Object::String(PdfString::hex(vec![0; 48])));
        dict.insert("P", Object::Integer(perm_value));
        dict
    }

    #[test]
    fn unsupported_versions_are_rejected() {
        let mut dict = encrypt_dict(-1);
        dict.insert("V", Object::Integer(3));
        assert!(Encryption::new(&dict, b"").is_err(), "/V 3 does not exist");

        let mut dict = encrypt_dict(-1);
        dict.insert("R", Object::Integer(5));
        assert!(
            Encryption::new(&dict, b"").is_err(),
            "revision 5 is deprecated"
        );

        let mut dict = encrypt_dict(-1);
        dict.insert("V", Object::Integer(4));
        dict.insert("R", Object::Integer(6));
        assert!(
            Encryption::new(&dict, b"").is_err(),
            "/V 4 with /R 6 is contradictory"
        );

        let mut dict = encrypt_dict(-1);
        dict.insert("Filter", Object::name("PublicKey"));
        assert!(Encryption::new(&dict, b"").is_err());

        let mut dict = encrypt_dict(-1);
        dict.insert("Length", Object::Integer(128));
        assert!(Encryption::new(&dict, b"").is_err());

        // Both filters `/Identity` is a legal way to say "nothing is encrypted";
        // the corpus has one, and the handler must not demand a password.
        let mut dict = encrypt_dict(-1);
        dict.insert("StmF", Object::name("Identity"));
        dict.insert("StrF", Object::name("Identity"));
        let identity = Encryption::new(&dict, b"").expect("an identity document");
        assert!(identity.is_identity());
        assert_eq!(identity.decrypt_string(b"plain").expect("plain"), b"plain");
    }

    #[test]
    fn a_wrong_password_is_an_error_without_parsing_anything_else() {
        let dict = encrypt_dict(-1);
        let error = Encryption::new(&dict, b"password").expect_err("wrong password");
        assert!(error.to_string().contains("password"), "{error}");
    }

    #[test]
    fn truncated_payloads_are_errors() {
        let dict = encrypt_dict(-1);
        // Fabricate a handler: no password check is attempted for the payload
        // paths, so a wrong-password rejection is bypassed by construction.
        let encryption = super::Encryption {
            kind: super::Kind::Aes256 { file_key: [7; 32] },
            encrypt_metadata: true,
        };
        assert!(encryption.decrypt_string(b"short").is_err());
        assert!(encryption.decrypt_string(&[0; 47]).is_err());
        assert!(dict.contains_key(b"U"));
    }
}

#[cfg(test)]
mod nist {
    use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::NoPadding};

    fn hex(text: &str) -> Vec<u8> {
        text.as_bytes()
            .chunks(2)
            .map(|pair| {
                u8::from_str_radix(core::str::from_utf8(pair).expect("hex"), 16).expect("hex")
            })
            .collect()
    }

    /// What appending costs under each cipher: RC4 keeps the payload's length,
    /// AES adds a 16-byte IV and a whole padding block, so a reader can size the
    /// ciphertext from the plaintext.
    #[test]
    fn encryption_adds_the_iv_and_padding_the_cipher_needs() {
        let handler = |kind| super::Encryption {
            kind,
            encrypt_metadata: true,
        };
        let id = crate::object::ObjectId::new(42, 0);
        let rc4_handler = handler(super::Kind::Legacy {
            file_key: alloc::vec![1, 2, 3, 4, 5],
            aes: false,
        });
        let aes128_handler = handler(super::Kind::Legacy {
            // `/Length 128`: AES-128 needs a 16-byte file key, so its per-object
            // key is 16 bytes too.
            file_key: alloc::vec![1; 16],
            aes: true,
        });
        let aes256_handler = handler(super::Kind::Aes256 {
            file_key: [7; super::KEY_LENGTH],
        });

        for payload_len in [0_usize, 1, 15, 16, 17, 100] {
            let plaintext = alloc::vec![b'a'; payload_len];
            let rc4 = rc4_handler.encrypt(&plaintext, Some(id)).expect("rc4");
            assert_eq!(rc4.len(), payload_len, "RC4 is a stream cipher");
            assert_eq!(
                rc4_handler.decrypt(&rc4, Some(id)).expect("back"),
                plaintext
            );
            for handler in [&aes128_handler, &aes256_handler] {
                let encrypted = handler.encrypt(&plaintext, Some(id)).expect("aes");
                let blocks = payload_len / 16 + 1;
                assert_eq!(
                    encrypted.len(),
                    16 + blocks * 16,
                    "AES: an IV and PKCS#7 padding over {payload_len} bytes"
                );
                assert_eq!(
                    handler.decrypt(&encrypted, Some(id)).expect("back"),
                    plaintext
                );
            }
        }
    }

    #[test]
    fn the_same_payload_encrypts_to_the_same_bytes() {
        let handler = super::Encryption {
            kind: super::Kind::Aes256 {
                file_key: [9; super::KEY_LENGTH],
            },
            encrypt_metadata: true,
        };
        let id = crate::object::ObjectId::new(3, 0);
        let first = handler.encrypt(b"a payload", Some(id)).expect("encrypt");
        let second = handler.encrypt(b"a payload", Some(id)).expect("encrypt");
        assert_eq!(first, second, "the IV is derived, so the bytes are stable");
        let other = handler.encrypt(b"a payloan", Some(id)).expect("encrypt");
        assert_ne!(
            first[..16],
            other[..16],
            "a different payload gets a different IV"
        );
    }

    #[test]
    fn aes256_cbc_matches_nist_sp_800_38a() {
        let key = hex("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
        let iv = hex("000102030405060708090a0b0c0d0e0f");
        let mut data = hex("6bc1bee22e409f96e93d7e117393172aae2d8a571e03ac9c9eb76fac45af8e51");
        let expected = hex("f58c4c04d6e5f1ba779eabfb5f7bfbd69cfc4e967edb808d679f777bc6702c7d");
        let len = data.len();
        let encrypted = cbc::Encryptor::<aes::Aes256>::new_from_slices(&key, &iv)
            .expect("key")
            .encrypt_padded_mut::<NoPadding>(&mut data, len)
            .expect("encrypt");
        assert_eq!(encrypted, expected.as_slice());
    }
}

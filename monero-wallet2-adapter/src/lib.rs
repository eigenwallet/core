//! Decrypts Monero `wallet2` `.keys` files (new JSON format only) and extracts
//! the spend / view secret keys.
//!
//! References: monero/src/wallet/wallet2.cpp (get_keys_file_data, load_keys_buf)
//! and monero/src/cryptonote_basic/account.cpp (xor_with_key_stream).
//!
//! Layers, outermost first:
//!
//!   1. `.keys` file: `IV(8) || LEB128-varint(len) || ChaCha20(plaintext)`.
//!      Key = `cn_slow_hash_v0(password)` iterated `kdf_rounds` times
//!      (monero's default is 1).
//!
//!   2. JSON (rapidjson-emitted; high bytes are raw, not UTF-8):
//!        `{ "key_data": <epee-encoded account_base>,
//!           "encrypted_secret_keys": 0|1, ... }`
//!
//!   3. When `encrypted_secret_keys == 1`, the spend and view secret keys
//!      inside `key_data` are XORed with a keystream:
//!        inner_key = cn_slow_hash_v0(outer_key || 'k')  (HASH_KEY_MEMORY)
//!        stream    = ChaCha20(inner_key, m_encryption_iv, 32 * (2 + multisig))
//!      First 32 bytes XOR `m_spend_secret_key`, next 32 XOR `m_view_secret_key`.

use anyhow::{anyhow, bail, Context, Result};
use chacha20::cipher::{KeyIvInit, StreamCipher};
use chacha20::ChaCha20Legacy;
use monero_epee::{Epee, EpeeEntry};

const CHACHA_KEY_SIZE: usize = 32;
const CHACHA_IV_SIZE: usize = 8;
const SECRET_KEY_SIZE: usize = 32;

/// Domain separator for inner-key derivation; `cryptonote_config.h`:`HASH_KEY_MEMORY`.
const HASH_KEY_MEMORY: u8 = b'k';

/// Spend and view secret keys recovered from a `.keys` file.
#[derive(Clone)]
pub struct WalletSecretKeys {
    pub spend_secret_key: [u8; SECRET_KEY_SIZE],
    pub view_secret_key: [u8; SECRET_KEY_SIZE],
}

/// Read a `.keys` file from disk and return its decrypted secret keys.
pub fn load_wallet_keys(path: &str, password: &str, kdf_rounds: u64) -> Result<WalletSecretKeys> {
    let buf = std::fs::read(path).with_context(|| format!("read {path}"))?;
    decrypt_wallet_keys(&buf, password, kdf_rounds)
}

/// Decrypt the raw bytes of a `.keys` file.
pub fn decrypt_wallet_keys(
    buf: &[u8],
    password: &str,
    kdf_rounds: u64,
) -> Result<WalletSecretKeys> {
    let outer_key = derive_chacha_key(password.as_bytes(), kdf_rounds);
    let outer_plaintext = decrypt_outer(buf, &outer_key)?;

    let key_data = extract_key_data_field(&outer_plaintext)?;
    let encrypted = extract_encrypted_secret_keys_flag(&outer_plaintext).unwrap_or(false);

    let (mut spend, mut view, enc_iv) = parse_account_keys_epee(&key_data)?;
    if encrypted {
        xor_inner_key_stream(&outer_key, &enc_iv, &mut spend, &mut view);
    }

    Ok(WalletSecretKeys {
        spend_secret_key: spend,
        view_secret_key: view,
    })
}

// ---- Outer container: IV + varint + ChaCha20 -------------------------------

fn decrypt_outer(buf: &[u8], key: &[u8; CHACHA_KEY_SIZE]) -> Result<Vec<u8>> {
    if buf.len() < CHACHA_IV_SIZE + 1 {
        bail!("file too short for IV + varint");
    }
    let iv: [u8; CHACHA_IV_SIZE] = buf[..CHACHA_IV_SIZE]
        .try_into()
        .expect("slice of CHACHA_IV_SIZE is an array of CHACHA_IV_SIZE");
    let (len, consumed) = read_leb128_varint(&buf[CHACHA_IV_SIZE..])?;
    let start = CHACHA_IV_SIZE + consumed;
    let end = start.checked_add(len).context("payload length overflow")?;
    if end != buf.len() {
        bail!(
            "malformed container: declared payload {} but {} bytes remain after header",
            len,
            buf.len().saturating_sub(start)
        );
    }

    let mut plaintext = buf[start..end].to_vec();
    let mut cipher = ChaCha20Legacy::new(key.into(), (&iv).into());
    cipher.apply_keystream(&mut plaintext);
    Ok(plaintext)
}

fn read_leb128_varint(buf: &[u8]) -> Result<(usize, usize)> {
    let mut value: u64 = 0;
    let mut shift = 0u32;
    for (i, &byte) in buf.iter().enumerate() {
        if shift >= 64 {
            bail!("varint overflow");
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok((value as usize, i + 1));
        }
        shift += 7;
    }
    bail!("truncated varint")
}

fn derive_chacha_key(password: &[u8], kdf_rounds: u64) -> [u8; CHACHA_KEY_SIZE] {
    let mut hash = cuprate_cryptonight::cryptonight_hash_v0(password);
    for _ in 1..kdf_rounds {
        hash = cuprate_cryptonight::cryptonight_hash_v0(&hash);
    }
    hash
}

// ---- JSON field extraction -------------------------------------------------
//
// The outer plaintext is JSON, but `key_data` is a rapidjson-emitted string
// that carries raw non-UTF-8 bytes verbatim (any byte ≥ 0x80 is written
// literally, rather than escaped). `serde_json` rejects that, so we scan for
// the two specific fields we need and unescape the `key_data` string with a
// minimal rapidjson-compatible decoder.

fn extract_key_data_field(json: &[u8]) -> Result<Vec<u8>> {
    let needle = br#""key_data":""#;
    let start = find(json, needle)
        .ok_or_else(|| anyhow!("\"key_data\" field not found in decrypted JSON"))?
        + needle.len();
    unescape_rapidjson_string(&json[start..])
}

fn extract_encrypted_secret_keys_flag(json: &[u8]) -> Option<bool> {
    let needle = br#""encrypted_secret_keys":"#;
    let start = find(json, needle)? + needle.len();
    match *json.get(start)? {
        b'0' => Some(false),
        b'1' => Some(true),
        _ => None,
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// Decode a JSON string body up to its closing `"`. Bytes ≥ 0x80 are treated
/// as raw payload (rapidjson emits them literally rather than as `\uXXXX`).
fn unescape_rapidjson_string(buf: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(256);
    let mut i = 0;
    while i < buf.len() {
        let c = buf[i];
        if c == b'"' {
            return Ok(out);
        }
        if c != b'\\' {
            out.push(c);
            i += 1;
            continue;
        }
        let esc = *buf.get(i + 1).context("truncated \\-escape")?;
        match esc {
            b'"' | b'\\' | b'/' => out.push(esc),
            b'b' => out.push(0x08),
            b'f' => out.push(0x0c),
            b'n' => out.push(b'\n'),
            b'r' => out.push(b'\r'),
            b't' => out.push(b'\t'),
            b'u' => {
                let hex = buf.get(i + 2..i + 6).context("truncated \\u escape")?;
                let hex = std::str::from_utf8(hex).context("\\u escape not ASCII hex")?;
                let code = u32::from_str_radix(hex, 16).context("\\u escape not hex")?;
                if code > 0xff {
                    // rapidjson only emits control bytes (< 0x20) as \u00XX in
                    // this context. Anything higher would imply real Unicode,
                    // which is not part of the binary account blob.
                    bail!("unexpected \\u escape > 0xff ({code:#06x}) in binary blob");
                }
                out.push(code as u8);
                i += 6;
                continue;
            }
            other => bail!("unknown escape \\{}", other as char),
        }
        i += 2;
    }
    bail!("unterminated JSON string")
}

// ---- Inner epee: account_base { m_keys { ... }, m_creation_timestamp } -----

fn parse_account_keys_epee(
    key_data: &[u8],
) -> Result<([u8; SECRET_KEY_SIZE], [u8; SECRET_KEY_SIZE], [u8; CHACHA_IV_SIZE])> {
    let mut decoder = Epee::new(key_data).map_err(|e| anyhow!("epee header: {e:?}"))?;
    let root = decoder.entry().map_err(|e| anyhow!("epee root: {e:?}"))?;
    let mut fields = root.fields().map_err(|e| anyhow!("epee fields: {e:?}"))?;

    while let Some(field) = fields.next() {
        let (key, entry) = field.map_err(|e| anyhow!("next field: {e:?}"))?;
        if key == b"m_keys" {
            return parse_m_keys(entry);
        }
    }
    bail!("m_keys field not found in account_base")
}

fn parse_m_keys<'e>(
    entry: EpeeEntry<'e, '_, &'e [u8]>,
) -> Result<([u8; SECRET_KEY_SIZE], [u8; SECRET_KEY_SIZE], [u8; CHACHA_IV_SIZE])> {
    let mut fields = entry.fields().map_err(|e| anyhow!("m_keys fields: {e:?}"))?;

    let mut spend: Option<[u8; SECRET_KEY_SIZE]> = None;
    let mut view: Option<[u8; SECRET_KEY_SIZE]> = None;
    // `KV_SERIALIZE_VAL_POD_AS_BLOB_OPT` in account.h defaults this to zeros.
    let mut enc_iv = [0u8; CHACHA_IV_SIZE];

    while let Some(field) = fields.next() {
        let (name, value) = field.map_err(|e| anyhow!("m_keys inner: {e:?}"))?;
        if name == b"m_spend_secret_key" {
            spend = Some(read_fixed_blob::<SECRET_KEY_SIZE>(value)?);
        } else if name == b"m_view_secret_key" {
            view = Some(read_fixed_blob::<SECRET_KEY_SIZE>(value)?);
        } else if name == b"m_encryption_iv" {
            enc_iv = read_fixed_blob::<CHACHA_IV_SIZE>(value)?;
        }
        // Other fields (m_account_address, m_multisig_keys) are skipped.
        // `EpeeEntry::Drop` advances the decoder past anything we don't read.
    }

    Ok((
        spend.context("m_spend_secret_key missing")?,
        view.context("m_view_secret_key missing")?,
        enc_iv,
    ))
}

fn read_fixed_blob<'e, const N: usize>(
    entry: EpeeEntry<'e, '_, &'e [u8]>,
) -> Result<[u8; N]> {
    let bytes: &[u8] = entry
        .to_fixed_len_str(N)
        .map_err(|e| anyhow!("to_fixed_len_str({N}): {e:?}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow!("fixed-len blob: expected {N} bytes, got {}", bytes.len()))
}

// ---- Inner keystream XOR (account_keys::xor_with_key_stream) --------------

fn xor_inner_key_stream(
    outer_key: &[u8; CHACHA_KEY_SIZE],
    enc_iv: &[u8; CHACHA_IV_SIZE],
    spend: &mut [u8; SECRET_KEY_SIZE],
    view: &mut [u8; SECRET_KEY_SIZE],
) {
    let inner_key = derive_inner_key(outer_key);
    let mut stream = [0u8; 2 * SECRET_KEY_SIZE];
    let mut cipher = ChaCha20Legacy::new((&inner_key).into(), enc_iv.into());
    cipher.apply_keystream(&mut stream);
    for i in 0..SECRET_KEY_SIZE {
        spend[i] ^= stream[i];
        view[i] ^= stream[SECRET_KEY_SIZE + i];
    }
}

/// `derive_key` from account.cpp: `cn_slow_hash_v0(base_key || HASH_KEY_MEMORY)`.
fn derive_inner_key(base_key: &[u8; CHACHA_KEY_SIZE]) -> [u8; CHACHA_KEY_SIZE] {
    let mut input = [0u8; CHACHA_KEY_SIZE + 1];
    input[..CHACHA_KEY_SIZE].copy_from_slice(base_key);
    input[CHACHA_KEY_SIZE] = HASH_KEY_MEMORY;
    cuprate_cryptonight::cryptonight_hash_v0(&input)
}


//! Signed device requests and replies (ADR 026).
//!
//! Over an untrusted network the device never sends its token. Both sides
//! derive a 32-byte key from it, `SHA-256(token)`, which is also what the
//! companion stores. Each request carries a fresh nonce and an HMAC-SHA-256
//! over the method, path, nonce and body hash. Each reply carries an HMAC over
//! the same nonce, the status and the body hash, so a reply cannot be forged,
//! altered or replayed for another request.
//!
//! This gives authenticity and integrity. It does not hide content: that is
//! TLS's job, and the device does not check the server's certificate.
use sha2::{Digest, Sha256};

pub const KEY_LEN: usize = 32;
pub const MAC_LEN: usize = 32;
pub const NONCE_LEN: usize = 16;
/// Hex lengths, as sent in headers.
pub const MAC_HEX: usize = 64;
pub const NONCE_HEX: usize = 32;

pub type Key = [u8; KEY_LEN];
pub type Mac = [u8; MAC_LEN];

/// The key for a device token.
pub fn key(token: &str) -> Key {
    Sha256::digest(token.as_bytes()).into()
}

/// The same key from the 64 hex digits the companion stores.
pub fn key_from_hex(hex: &str) -> Option<Key> {
    let mut out = [0u8; KEY_LEN];
    decode_hex(hex, &mut out)?;
    Some(out)
}

/// HMAC-SHA-256 (RFC 2104) over the concatenation of `parts`. The key is
/// always 32 bytes, so it never needs hashing down.
pub fn hmac(key: &Key, parts: &[&[u8]]) -> Mac {
    let mut inner = Sha256::new();
    let mut outer = Sha256::new();
    let mut ipad = [0x36u8; 64];
    let mut opad = [0x5cu8; 64];
    for i in 0..KEY_LEN {
        ipad[i] ^= key[i];
        opad[i] ^= key[i];
    }
    inner.update(ipad);
    for p in parts {
        inner.update(p);
    }
    outer.update(opad);
    outer.update(inner.finalize());
    outer.finalize().into()
}

pub fn hex(bytes: &[u8], out: &mut [u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (i, b) in bytes.iter().enumerate() {
        out[i * 2] = DIGITS[(b >> 4) as usize];
        out[i * 2 + 1] = DIGITS[(b & 15) as usize];
    }
}

/// Lower-case hex only. `None` for any other character or a wrong length.
pub fn decode_hex(s: &str, out: &mut [u8]) -> Option<()> {
    let b = s.as_bytes();
    if b.len() != out.len() * 2 {
        return None;
    }
    let nibble = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    for (i, o) in out.iter_mut().enumerate() {
        *o = nibble(b[i * 2])? << 4 | nibble(b[i * 2 + 1])?;
    }
    Some(())
}

/// Compares in time that does not depend on where the bytes differ.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

/// MAC for a request. `nonce` is the 32 hex digits sent in `X3-Nonce`.
pub fn request_mac(key: &Key, method: &str, path: &str, nonce: &str, body: &[u8]) -> Mac {
    let mut body_hash = [0u8; 64];
    hex(&Sha256::digest(body), &mut body_hash);
    hmac(
        key,
        &[
            b"x3req1\n",
            method.as_bytes(),
            b"\n",
            path.as_bytes(),
            b"\n",
            nonce.as_bytes(),
            b"\n",
            &body_hash,
        ],
    )
}

/// MAC for a reply to the request that carried `nonce`. `body_sha256` is the
/// SHA-256 of the whole reply body (of nothing, for a 204).
pub fn response_mac(key: &Key, nonce: &str, status: u16, body_sha256: &[u8; 32]) -> Mac {
    let mut body_hash = [0u8; 64];
    hex(body_sha256, &mut body_hash);
    let mut code = [0u8; 3];
    code[0] = b'0' + (status / 100 % 10) as u8;
    code[1] = b'0' + (status / 10 % 10) as u8;
    code[2] = b'0' + (status % 10) as u8;
    hmac(
        key,
        &[
            b"x3res1\n",
            nonce.as_bytes(),
            b"\n",
            &code,
            b"\n",
            &body_hash,
        ],
    )
}

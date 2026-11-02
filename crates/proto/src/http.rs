//! HTTP/1.1 framing for one response per connection. No redirects or encodings.
use crate::{
    Error, Result,
    config::{Address, token},
    formatted, job_id, number, text,
};
/// Room for Cloudflare's own headers (about 900 B) on top of the companion's.
pub const HEAD_CAP: usize = 2048;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResponseHead {
    pub status: u16,
    pub content_length: usize,
    pub head_length: usize,
    /// The `X3-Mac` header, if the reply carried one (ADR 026).
    pub mac: Option<crate::sign::Mac>,
}
fn header_name(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}
impl ResponseHead {
    /// Accepts a receive buffer which may already contain body bytes. `None`
    /// means more header bytes are needed. The route cap applies before allocation.
    pub fn parse(bytes: &[u8], body_cap: usize) -> Result<Option<Self>> {
        let Some(end) = bytes
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|n| n + 4)
        else {
            return if bytes.len() >= HEAD_CAP {
                Err(Error::TooLarge)
            } else {
                Ok(None)
            };
        };
        if end > HEAD_CAP {
            return Err(Error::TooLarge);
        }
        let mut lines = text(&bytes[..end - 2])?.split_terminator("\r\n");
        let status_line = lines.next().ok_or(Error::Invalid)?;
        let rest = status_line
            .strip_prefix("HTTP/1.1 ")
            .ok_or(Error::Invalid)?;
        let (code, reason) = rest.split_once(' ').ok_or(Error::Invalid)?;
        if code.len() != 3 || reason.bytes().any(|b| !(32..=126).contains(&b)) {
            return Err(Error::Invalid);
        }
        let status = number(code)? as u16;
        if !(200..=599).contains(&status) || (300..=399).contains(&status) {
            return Err(Error::Invalid);
        }
        let mut length = None;
        let mut mac = None;
        for line in lines {
            if line.is_empty() {
                continue;
            }
            let (name, value) = line.split_once(':').ok_or(Error::Invalid)?;
            if !header_name(name)
                || value
                    .bytes()
                    .any(|b| !(32..=126).contains(&b) && b != b'\t')
            {
                return Err(Error::Invalid);
            }
            if name.eq_ignore_ascii_case("transfer-encoding")
                || name.eq_ignore_ascii_case("content-encoding")
            {
                return Err(Error::Invalid);
            }
            if name.eq_ignore_ascii_case("x3-mac") {
                let mut m = [0u8; crate::sign::MAC_LEN];
                if mac.is_some()
                    || crate::sign::decode_hex(value.trim_matches([' ', '\t']), &mut m).is_none()
                {
                    return Err(Error::Invalid);
                }
                mac = Some(m);
            }
            if name.eq_ignore_ascii_case("content-length") {
                if length.is_some() {
                    return Err(Error::Invalid);
                }
                length = Some(
                    usize::try_from(number(value.trim_matches([' ', '\t']))?)
                        .map_err(|_| Error::TooLarge)?,
                );
            }
        }
        let content_length = if status == 204 {
            length.unwrap_or(0)
        } else {
            length.ok_or(Error::Invalid)?
        };
        if content_length > body_cap || bytes.len() - end > content_length {
            return Err(Error::TooLarge);
        }
        if status == 204 && content_length != 0 {
            return Err(Error::Invalid);
        }
        Ok(Some(Self {
            status,
            content_length,
            head_length: end,
            mac,
        }))
    }
}
/// Track received body length independently of TCP fragment boundaries.
#[derive(Debug)]
pub struct Body {
    remaining: usize,
}
impl Body {
    pub fn new(head: ResponseHead) -> Self {
        Self {
            remaining: head.content_length,
        }
    }
    pub fn accept(&mut self, bytes: &[u8]) -> Result<()> {
        self.remaining = self
            .remaining
            .checked_sub(bytes.len())
            .ok_or(Error::TooLarge)?;
        Ok(())
    }
    pub fn finish(self) -> Result<()> {
        if self.remaining == 0 {
            Ok(())
        } else {
            Err(Error::Truncated)
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Route<'a> {
    Hello,
    Jobs,
    Payload(&'a str),
    Ack(&'a str),
}
/// Writes the request head only. Caller sends exactly `body_length` bytes next.
pub fn request(
    address: Address,
    bearer: &str,
    route: Route<'_>,
    body_length: usize,
    out: &mut [u8],
) -> Result<usize> {
    token(bearer)?;
    let (method, path, suffix) = match route {
        Route::Hello => ("POST", "hello", ""),
        Route::Jobs => ("GET", "jobs", ""),
        Route::Payload(id) => {
            job_id(id)?;
            ("GET", id, "/payload")
        }
        Route::Ack(id) => {
            job_id(id)?;
            ("POST", id, "/ack")
        }
    };
    let limit = match route {
        Route::Hello => 161,
        Route::Ack(_) => 193,
        _ => 0,
    };
    if body_length > limit || address.port == 0 {
        return Err(Error::Invalid);
    }
    let prefix = if matches!(route, Route::Payload(_) | Route::Ack(_)) {
        "jobs/"
    } else {
        ""
    };
    let [a, b, c, d] = address.ip;
    formatted(
        out,
        format_args!(
            "{method} /device/v1/{prefix}{path}{suffix} HTTP/1.1\r\nHost: {a}.{b}.{c}.{d}:{}\r\nAuthorization: Bearer {bearer}\r\nConnection: close\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {body_length}\r\n\r\n",
            address.port
        ),
    )
}

/// The 32 hex digits sent as `X3-Nonce`.
pub fn nonce_hex(nonce: &[u8; crate::sign::NONCE_LEN]) -> [u8; crate::sign::NONCE_HEX] {
    let mut out = [0u8; crate::sign::NONCE_HEX];
    crate::sign::hex(nonce, &mut out);
    out
}

/// Writes the head of a signed request for the internet form (ADR 026). No
/// token is sent: `Authorization: X3-HMAC <mac>` covers the method, path, nonce
/// and `body`. `body` is hashed here; the caller sends exactly these bytes next.
/// No `Connection: close`: the connection is kept for the next request.
pub fn signed_request(
    remote: crate::config::Remote<'_>,
    access: Option<crate::config::Access<'_>>,
    key: &crate::sign::Key,
    nonce: &[u8; crate::sign::NONCE_LEN],
    route: Route<'_>,
    body: &[u8],
    out: &mut [u8],
) -> Result<usize> {
    let (host, port) = (remote.host, remote.port);
    let (method, path, suffix, prefix) = match route {
        Route::Hello => ("POST", "hello", "", ""),
        Route::Jobs => ("GET", "jobs", "", ""),
        Route::Payload(id) => {
            job_id(id)?;
            ("GET", id, "/payload", "jobs/")
        }
        Route::Ack(id) => {
            job_id(id)?;
            ("POST", id, "/ack", "jobs/")
        }
    };
    let limit = match route {
        Route::Hello => 161,
        Route::Ack(_) => 193,
        _ => 0,
    };
    if body.len() > limit || port == 0 {
        return Err(Error::Invalid);
    }
    let mut full = [0u8; 96];
    let n = formatted(&mut full, format_args!("/device/v1/{prefix}{path}{suffix}"))?;
    let full = text(&full[..n])?;
    let nonce = nonce_hex(nonce);
    let nonce = text(&nonce)?;
    let mac = crate::sign::request_mac(key, method, full, nonce, body);
    let mut mac_hex = [0u8; crate::sign::MAC_HEX];
    crate::sign::hex(&mac, &mut mac_hex);
    let mac_hex = text(&mac_hex)?;
    // Cloudflare Access service token, if configured: lets the request past the gate.
    let mut gate = [0u8; 320];
    let gate_len = match access {
        Some(a) => formatted(
            &mut gate,
            format_args!(
                "CF-Access-Client-Id: {}\r\nCF-Access-Client-Secret: {}\r\n",
                a.id, a.secret
            ),
        )?,
        None => 0,
    };
    let gate = text(&gate[..gate_len])?;
    let host_port = if port == 443 { None } else { Some(port) };
    match host_port {
        None => formatted(
            out,
            format_args!(
                "{method} {full} HTTP/1.1\r\nHost: {host}\r\n{gate}X3-Nonce: {nonce}\r\nAuthorization: X3-HMAC {mac_hex}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ),
        ),
        Some(p) => formatted(
            out,
            format_args!(
                "{method} {full} HTTP/1.1\r\nHost: {host}:{p}\r\n{gate}X3-Nonce: {nonce}\r\nAuthorization: X3-HMAC {mac_hex}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
                body.len()
            ),
        ),
    }
}

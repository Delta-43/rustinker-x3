//! Wi-Fi setup page (ADR 025): the request parser, form decoder, page text
//! and hotspot password. No hardware and no allocation; the firmware only
//! moves bytes between the socket and these functions.
//!
//! Everything here treats the request as hostile. Heads and bodies are capped,
//! one request is read per connection, and nothing from the form is echoed
//! into the page unescaped.
use core::fmt::{self, Write};

pub const HEAD_CAP: usize = 1024;
pub const BODY_CAP: usize = 256;
/// Largest request the firmware ever buffers.
pub const REQUEST_CAP: usize = HEAD_CAP + BODY_CAP;
/// How many scanned networks the page lists.
pub const LISTED: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Parse<'a> {
    /// Read more bytes.
    Incomplete,
    /// Close the connection with a 400.
    Invalid,
    Request {
        method: Method,
        path: &'a str,
        body: &'a [u8],
    },
}

/// Reads the head and, for a POST, the body. `buf` is everything received so
/// far. It answers `Incomplete` until the whole request is there.
pub fn parse_request(buf: &[u8]) -> Parse<'_> {
    let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|n| n + 4) else {
        return if buf.len() >= HEAD_CAP {
            Parse::Invalid
        } else {
            Parse::Incomplete
        };
    };
    if end > HEAD_CAP {
        return Parse::Invalid;
    }
    let Ok(head) = core::str::from_utf8(&buf[..end - 4]) else {
        return Parse::Invalid;
    };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (Some(verb), Some(path), Some("HTTP/1.1"), None) =
        (first.next(), first.next(), first.next(), first.next())
    else {
        return Parse::Invalid;
    };
    let method = match verb {
        "GET" => Method::Get,
        "POST" => Method::Post,
        _ => return Parse::Invalid,
    };
    if !path.starts_with('/') || path.len() > 64 || path.bytes().any(|b| b <= b' ' || b == 0x7f) {
        return Parse::Invalid;
    }
    let mut length = None;
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Parse::Invalid;
        };
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Parse::Invalid;
        }
        if name.eq_ignore_ascii_case("content-length") {
            let v = value.trim();
            if length.is_some() || v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                return Parse::Invalid;
            }
            match v.parse::<usize>() {
                Ok(n) if n <= BODY_CAP => length = Some(n),
                _ => return Parse::Invalid,
            }
        }
    }
    let want = match (method, length) {
        (Method::Get, None | Some(0)) => 0,
        (Method::Get, Some(_)) => return Parse::Invalid,
        (Method::Post, Some(n)) => n,
        (Method::Post, None) => return Parse::Invalid,
    };
    match buf.len() - end {
        have if have < want => Parse::Incomplete,
        have if have > want => Parse::Invalid,
        _ => Parse::Request {
            method,
            path,
            body: &buf[end..],
        },
    }
}

/// The name and password from the form. No `Debug`: it holds a password.
pub struct Credentials {
    ssid: [u8; 32],
    ssid_len: usize,
    psk: [u8; 63],
    psk_len: usize,
}
impl Credentials {
    pub fn ssid(&self) -> &str {
        core::str::from_utf8(&self.ssid[..self.ssid_len]).unwrap_or("")
    }
    pub fn psk(&self) -> &str {
        core::str::from_utf8(&self.psk[..self.psk_len]).unwrap_or("")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormError {
    /// Not a form we can read.
    Malformed,
    /// Name must be 1–32 bytes.
    Name,
    /// Password must be 8–63 bytes.
    Password,
}

fn hex(b: u8) -> Option<u8> {
    (b as char).to_digit(16).map(|d| d as u8)
}

/// Percent-decodes one form value (`+` is a space) into `out`. Returns the
/// bytes written, or `None` for a bad escape or if `out` is too small.
fn decode(value: &str, out: &mut [u8]) -> Option<usize> {
    let mut n = 0;
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        let d = match b {
            b'+' => b' ',
            b'%' => hex(bytes.next()?)? << 4 | hex(bytes.next()?)?,
            _ => b,
        };
        *out.get_mut(n)? = d;
        n += 1;
    }
    Some(n)
}

/// Reads `ssid=…&psk=…` (any order, each exactly once, nothing else).
pub fn form_credentials(body: &[u8]) -> Result<Credentials, FormError> {
    let body = core::str::from_utf8(body).map_err(|_| FormError::Malformed)?;
    let mut c = Credentials {
        ssid: [0; 32],
        ssid_len: 0,
        psk: [0; 63],
        psk_len: 0,
    };
    let (mut got_ssid, mut got_psk) = (false, false);
    // Wide enough for any legal field; a longer one fails the length check.
    let mut scratch = [0u8; 128];
    for pair in body.split('&') {
        let (key, value) = pair.split_once('=').ok_or(FormError::Malformed)?;
        let n = decode(value, &mut scratch).ok_or(match key {
            "ssid" => FormError::Name,
            "psk" => FormError::Password,
            _ => FormError::Malformed,
        })?;
        let value = &scratch[..n];
        let text = core::str::from_utf8(value).map_err(|_| FormError::Malformed)?;
        if text.chars().any(char::is_control) {
            return Err(FormError::Malformed);
        }
        match key {
            "ssid" if !got_ssid => {
                if !(1..=32).contains(&n) {
                    return Err(FormError::Name);
                }
                c.ssid[..n].copy_from_slice(value);
                c.ssid_len = n;
                got_ssid = true;
            }
            "psk" if !got_psk => {
                if !(8..=63).contains(&n) {
                    return Err(FormError::Password);
                }
                c.psk[..n].copy_from_slice(value);
                c.psk_len = n;
                got_psk = true;
            }
            _ => return Err(FormError::Malformed),
        }
    }
    if got_ssid && got_psk {
        Ok(c)
    } else {
        Err(FormError::Malformed)
    }
}

/// Writes `s` with the characters HTML cares about escaped.
struct Escaped<'a>(&'a str);
impl fmt::Display for Escaped<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.0.chars() {
            match c {
                '&' => f.write_str("&amp;")?,
                '<' => f.write_str("&lt;")?,
                '>' => f.write_str("&gt;")?,
                '"' => f.write_str("&quot;")?,
                '\'' => f.write_str("&#39;")?,
                c if c.is_control() => f.write_char(' ')?,
                c => f.write_char(c)?,
            }
        }
        Ok(())
    }
}

/// What the page says above the form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notice {
    None,
    Name,
    Password,
    Malformed,
}

const STYLE: &str = "body{font:18px/1.5 Georgia,serif;margin:1.2em auto;max-width:28em;padding:0 1em}\
input{font:inherit;width:100%;box-sizing:border-box;padding:.5em;margin:.2em 0 1em}\
button{font:inherit;padding:.6em 1.4em}.n{color:#a00}";

/// The setup form. `networks` are the names the X3 saw; each is offered as a
/// suggestion. Output goes to `w`; on error discard what was written.
pub fn page<'n>(
    w: &mut impl Write,
    networks: impl IntoIterator<Item = &'n str>,
    notice: Notice,
) -> fmt::Result {
    write!(
        w,
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">\
         <title>X3 Wi-Fi</title><style>{STYLE}</style><h1>Add a Wi-Fi network</h1>"
    )?;
    let note = match notice {
        Notice::None => "",
        Notice::Name => "The name must be 1 to 32 characters.",
        Notice::Password => "The password must be 8 to 63 characters.",
        Notice::Malformed => "That did not work. Try again.",
    };
    if !note.is_empty() {
        write!(w, "<p class=n>{note}</p>")?;
    }
    write!(
        w,
        "<form method=post action=/save accept-charset=utf-8>\
         <label>Network name<input name=ssid list=n maxlength=32 required autocapitalize=none autocorrect=off></label>\
         <datalist id=n>"
    )?;
    for name in networks.into_iter().take(LISTED) {
        write!(w, "<option value=\"{}\">", Escaped(name))?;
    }
    write!(
        w,
        "</datalist><label>Password<input name=psk type=password minlength=8 maxlength=63 required></label>\
         <button>Save</button></form>"
    )
}

/// Shown after a good save. The phone has lost the link by the time it is read.
pub const DONE: &str = "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\">\
<title>X3 Wi-Fi</title><body style=\"font:18px/1.5 Georgia,serif;margin:1.2em auto;max-width:28em;padding:0 1em\">\
<h1>Saved</h1><p>The X3 is joining the network now. Look at its screen for the result.</p>";

/// Characters for the hotspot password: no 0/o, 1/l/i, which are hard to read
/// off a screen. 32 symbols, so 5 bits per character.
const ALPHABET: &[u8; 32] = b"23456789abcdefghjkmnpqrstuvwxyz2";
pub const PASSWORD_LEN: usize = 10;

/// Fills `out` from `random`, five bits per character.
pub fn password(mut random: impl FnMut() -> u32, out: &mut [u8; PASSWORD_LEN]) {
    for slot in out.iter_mut() {
        *slot = ALPHABET[(random() & 31) as usize];
    }
}

/// The text a phone camera reads to join a Wi-Fi network:
/// `WIFI:T:WPA;S:name;P:password;;`. Special characters are escaped.
pub fn join_qr_text(w: &mut impl Write, ssid: &str, password: &str) -> fmt::Result {
    fn field(w: &mut impl Write, s: &str) -> fmt::Result {
        for c in s.chars() {
            if matches!(c, '\\' | ';' | ',' | ':' | '"') {
                w.write_char('\\')?;
            }
            w.write_char(c)?;
        }
        Ok(())
    }
    w.write_str("WIFI:T:WPA;S:")?;
    field(w, ssid)?;
    w.write_str(";P:")?;
    field(w, password)?;
    w.write_str(";;")
}

/// The order to try saved networks in: the chosen one first (if it is saved),
/// then those the scan saw, strongest first, then the rest in file order (a
/// hidden network does not show in a scan). The chosen network goes first even
/// when the scan missed it; the others stay as a fallback.
/// `saved` has at most four names; `seen` is (name, signal in dBm) pairs.
/// Returns the indexes into `saved` and how many.
pub fn join_order(
    saved: &[&str],
    seen: &[(&str, i8)],
    chosen: Option<&str>,
) -> ([usize; 4], usize) {
    let n = saved.len().min(4);
    let mut order = [0usize; 4];
    for (i, slot) in order[..n].iter_mut().enumerate() {
        *slot = i;
    }
    let signal = |i: usize| {
        seen.iter()
            .filter(|(name, _)| *name == saved[i])
            .map(|(_, rssi)| *rssi)
            .max()
    };
    let key = |i: usize| {
        (
            chosen != Some(saved[i]),
            signal(i).is_none(),
            core::cmp::Reverse(signal(i)),
            i,
        )
    };
    // Insertion sort: at most four items, and `core` has no slice sort.
    for i in 1..n {
        let mut j = i;
        while j > 0 && key(order[j]) < key(order[j - 1]) {
            order.swap(j, j - 1);
            j -= 1;
        }
    }
    (order, n)
}

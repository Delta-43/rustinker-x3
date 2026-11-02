//! SD configuration. Secret-bearing types deliberately do not implement Debug.
use crate::{Error, Result, number, text};
fn pair<'a>(bytes: &'a [u8], first: &str, second: &str) -> Result<(&'a str, &'a str)> {
    if bytes.len() > 512 {
        return Err(Error::TooLarge);
    }
    let s = text(bytes)?;
    if !s.ends_with('\n') {
        return Err(Error::Truncated);
    }
    if s.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Error::Invalid);
    }
    let mut a = None;
    let mut b = None;
    for line in s.split_terminator('\n') {
        let (k, v) = line.split_once('=').ok_or(Error::Invalid)?;
        if k == first && a.is_none() {
            a = Some(v);
        } else if k == second && b.is_none() {
            b = Some(v);
        } else {
            return Err(Error::Invalid);
        }
    }
    Ok((a.ok_or(Error::Invalid)?, b.ok_or(Error::Invalid)?))
}
#[derive(Clone, Copy)]
pub struct Wifi<'a> {
    pub ssid: &'a str,
    pub psk: &'a str,
}
impl<'a> Wifi<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let (ssid, psk) = pair(bytes, "ssid", "psk")?;
        if !(1..=32).contains(&ssid.len()) || !(8..=63).contains(&psk.len()) {
            return Err(Error::Invalid);
        }
        Ok(Self { ssid, psk })
    }
}
/// How many networks `WIFI.CFG` keeps (ADR 025).
pub const MAX_NETWORKS: usize = 4;

fn valid_network(ssid: &str, psk: &str) -> bool {
    (1..=32).contains(&ssid.len())
        && (8..=63).contains(&psk.len())
        && !ssid.chars().chain(psk.chars()).any(char::is_control)
}

/// Up to four saved networks, in file order. One `ssid=` line, then its
/// `psk=` line, repeated. A file with one pair is the old single-network
/// format. No `Debug`: it holds passwords.
pub struct WifiList<'a> {
    items: [Option<Wifi<'a>>; MAX_NETWORKS],
    len: usize,
}
impl<'a> WifiList<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 512 {
            return Err(Error::TooLarge);
        }
        let s = text(bytes)?;
        if !s.ends_with('\n') {
            return Err(Error::Truncated);
        }
        if s.chars().any(|c| c.is_control() && c != '\n') {
            return Err(Error::Invalid);
        }
        let mut list = Self {
            items: [None; MAX_NETWORKS],
            len: 0,
        };
        let mut lines = s.split_terminator('\n');
        while let Some(first) = lines.next() {
            let ssid = first.strip_prefix("ssid=").ok_or(Error::Invalid)?;
            let psk = lines
                .next()
                .and_then(|l| l.strip_prefix("psk="))
                .ok_or(Error::Invalid)?;
            if !valid_network(ssid, psk) || list.len == MAX_NETWORKS {
                return Err(Error::Invalid);
            }
            list.items[list.len] = Some(Wifi { ssid, psk });
            list.len += 1;
        }
        if list.len == 0 {
            return Err(Error::Invalid);
        }
        Ok(list)
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = &Wifi<'a>> {
        self.items[..self.len].iter().flatten()
    }
    pub fn find(&self, ssid: &str) -> Option<&Wifi<'a>> {
        self.iter().find(|w| w.ssid == ssid)
    }
}

/// Writes the file that results from saving `ssid`/`psk` after the networks in
/// `existing` (none if the card has no file or it is unreadable). A name that
/// is already saved is replaced and moves to the end; a fifth network drops the
/// oldest. Returns the length written to `out`, which must hold 512 bytes.
pub fn save_network(
    existing: Option<&WifiList<'_>>,
    ssid: &str,
    psk: &str,
    out: &mut [u8; 512],
) -> Result<usize> {
    if !valid_network(ssid, psk) {
        return Err(Error::Invalid);
    }
    let kept = existing.map_or(0, |l| l.iter().filter(|w| w.ssid != ssid).count());
    let skip = (kept + 1).saturating_sub(MAX_NETWORKS);
    let mut w = Cursor { bytes: out, len: 0 };
    let others = existing
        .into_iter()
        .flat_map(|l| l.iter())
        .filter(|w| w.ssid != ssid)
        .skip(skip);
    for n in others.map(|w| (w.ssid, w.psk)).chain([(ssid, psk)]) {
        core::fmt::write(&mut w, format_args!("ssid={}\npsk={}\n", n.0, n.1))
            .map_err(|_| Error::BufferTooSmall)?;
    }
    Ok(w.len)
}
/// The network Receive tries first, from `/X3/WIFI.PIN`: one `ssid=` line.
/// No file, or an empty one, means automatic (the strongest saved network).
/// Holds a network name only, never a password.
pub fn chosen_network(bytes: &[u8]) -> Result<Option<&str>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    if bytes.len() > 64 {
        return Err(Error::TooLarge);
    }
    let s = text(bytes)?;
    let line = s.strip_suffix('\n').ok_or(Error::Truncated)?;
    let ssid = line.strip_prefix("ssid=").ok_or(Error::Invalid)?;
    if !(1..=32).contains(&ssid.len()) || ssid.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    Ok(Some(ssid))
}

/// Writes the `WIFI.PIN` file for `ssid` into `out`; returns its length.
pub fn write_chosen_network(ssid: &str, out: &mut [u8; 64]) -> Result<usize> {
    if !(1..=32).contains(&ssid.len()) || ssid.chars().any(char::is_control) {
        return Err(Error::Invalid);
    }
    let n = ssid.len() + 6;
    out[..5].copy_from_slice(b"ssid=");
    out[5..n - 1].copy_from_slice(ssid.as_bytes());
    out[n - 1] = b'\n';
    Ok(n)
}

struct Cursor<'b> {
    bytes: &'b mut [u8; 512],
    len: usize,
}
impl core::fmt::Write for Cursor<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(core::fmt::Error)?;
        let dst = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        dst.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Address {
    pub ip: [u8; 4],
    pub port: u16,
}
impl Address {
    pub fn parse(url: &str) -> Result<Self> {
        let s = url.strip_prefix("http://").ok_or(Error::Invalid)?;
        let (ip, port) = s.split_once(':').ok_or(Error::Invalid)?;
        let port = u16::try_from(number(port)?).map_err(|_| Error::Invalid)?;
        if port == 0 {
            return Err(Error::Invalid);
        }
        let mut parts = ip.split('.');
        let mut octets = [0; 4];
        for octet in &mut octets {
            let p = parts.next().ok_or(Error::Invalid)?;
            if p.len() > 1 && p.starts_with('0') {
                return Err(Error::Invalid);
            }
            *octet = u8::try_from(number(p)?).map_err(|_| Error::Invalid)?;
        }
        if parts.next().is_some() {
            return Err(Error::Invalid);
        }
        Ok(Self { ip: octets, port })
    }
}
pub fn token(s: &str) -> Result<&str> {
    if s.len() != 43
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(Error::Invalid);
    }
    Ok(s)
}
/// A companion reached over the internet by name: `https://host[:port]`. The
/// device does not check its certificate; requests are signed instead
/// (`sign`, ADR 026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Remote<'a> {
    pub host: &'a str,
    pub port: u16,
}
impl<'a> Remote<'a> {
    pub fn parse(url: &'a str) -> Result<Self> {
        let s = url.strip_prefix("https://").ok_or(Error::Invalid)?;
        let (host, port) = match s.split_once(':') {
            Some((h, p)) => (h, u16::try_from(number(p)?).map_err(|_| Error::Invalid)?),
            None => (s, 443),
        };
        let label_ok = |l: &str| {
            (1..=63).contains(&l.len())
                && !l.starts_with('-')
                && !l.ends_with('-')
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        };
        // Lower-case letters, digits, hyphens and dots; at least two labels; and
        // not a bare IPv4 address (that is the `http://` form).
        if port == 0
            || host.len() > 60
            || !host.contains('.')
            || !host.split('.').all(label_ok)
            || host
                .split('.')
                .all(|l| l.bytes().all(|b| b.is_ascii_digit()))
        {
            return Err(Error::Invalid);
        }
        Ok(Self { host, port })
    }
}
/// `SERVER.CFG`. `url=` is either the LAN form (`http://192.168.1.20:8788`, the
/// device sends its token) or the internet form (`https://host`, requests are
/// signed). With the internet form, an optional `lan=http://ip:port` line is
/// tried first, so the home network does not need the internet.
pub struct Companion<'a> {
    pub address: Option<Address>,
    pub remote: Option<Remote<'a>>,
    /// A Cloudflare Access service token for the internet form, sent as two
    /// headers so the Access gate lets the request through.
    pub access: Option<Access<'a>>,
    pub token: &'a str,
}
/// `access_id=` and `access_secret=`: a Cloudflare Access service token.
#[derive(Clone, Copy)]
pub struct Access<'a> {
    pub id: &'a str,
    pub secret: &'a str,
}
fn access_value(s: &str, max: usize) -> bool {
    (1..=max).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
impl<'a> Companion<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        if bytes.len() > 512 {
            return Err(Error::TooLarge);
        }
        let s = text(bytes)?;
        if !s.ends_with('\n') {
            return Err(Error::Truncated);
        }
        if s.chars().any(|c| c.is_control() && c != '\n') {
            return Err(Error::Invalid);
        }
        let (mut url, mut lan, mut tok) = (None, None, None);
        let (mut access_id, mut access_secret) = (None, None);
        for line in s.split_terminator('\n') {
            let (k, v) = line.split_once('=').ok_or(Error::Invalid)?;
            let slot = match k {
                "url" => &mut url,
                "lan" => &mut lan,
                "token" => &mut tok,
                "access_id" => &mut access_id,
                "access_secret" => &mut access_secret,
                _ => return Err(Error::Invalid),
            };
            if slot.replace(v).is_some() {
                return Err(Error::Invalid);
            }
        }
        let (url, tok) = (url.ok_or(Error::Invalid)?, tok.ok_or(Error::Invalid)?);
        let token = token(tok)?;
        let access = match (access_id, access_secret) {
            (None, None) => None,
            (Some(id), Some(secret)) if access_value(id, 100) && access_value(secret, 128) => {
                Some(Access { id, secret })
            }
            _ => return Err(Error::Invalid),
        };
        if url.starts_with("https://") {
            Ok(Self {
                address: lan.map(Address::parse).transpose()?,
                remote: Some(Remote::parse(url)?),
                access,
                token,
            })
        } else if lan.is_some() || access.is_some() {
            Err(Error::Invalid)
        } else {
            Ok(Self {
                address: Some(Address::parse(url)?),
                remote: None,
                access: None,
                token,
            })
        }
    }
}

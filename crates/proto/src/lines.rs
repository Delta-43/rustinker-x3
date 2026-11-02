//! Complete-body validation: callers cannot act on a partially valid job batch.
use crate::{
    ARTICLE_CAP, Error, JOBS_CAP, LINE_CAP, Result, TOKEN_CAP, formatted, hash, item_id, job_id,
    number, text,
};

pub fn validate_lines(bytes: &[u8], cap: usize) -> Result<&str> {
    if bytes.len() > cap {
        return Err(Error::TooLarge);
    }
    let s = text(bytes)?;
    if !s.ends_with('\n') {
        return Err(Error::Truncated);
    }
    if s.chars().any(|c| c.is_control() && c != '\n') {
        return Err(Error::Invalid);
    }
    for line in s.split_terminator('\n') {
        if line.len() > LINE_CAP {
            return Err(Error::TooLarge);
        }
        if line.is_empty() || line.starts_with(' ') || line.ends_with(' ') || line.contains("  ") {
            return Err(Error::Invalid);
        }
    }
    Ok(s)
}
fn single(bytes: &[u8], limit: usize) -> Result<&str> {
    let s = validate_lines(bytes, limit + 1)?;
    let line = s.strip_suffix('\n').ok_or(Error::Truncated)?;
    if line.contains('\n') {
        return Err(Error::Invalid);
    }
    Ok(line)
}
fn fields<const N: usize>(s: &str) -> Result<[&str; N]> {
    let mut out = [""; N];
    let mut iter = s.split(' ');
    for slot in &mut out {
        *slot = iter
            .next()
            .filter(|s| !s.is_empty())
            .ok_or(Error::Invalid)?;
    }
    if iter.next().is_some() {
        return Err(Error::Invalid);
    }
    Ok(out)
}
fn key<'a>(s: &'a str, prefix: &str) -> Result<&'a str> {
    s.strip_prefix(prefix).ok_or(Error::Invalid)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hello<'a> {
    pub firmware: &'a str,
    pub width: u16,
    pub height: u16,
    pub formats: &'a str,
    pub free_kib: u64,
    pub battery: Option<u8>,
}
impl<'a> Hello<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let f = fields::<8>(single(bytes, 160)?)?;
        if f[0] != "hello" || f[1] != "1" {
            return Err(Error::Invalid);
        }
        let firmware = key(f[2], "fw=")?;
        if firmware.is_empty()
            || !firmware
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b))
        {
            return Err(Error::Invalid);
        }
        let width = u16::try_from(number(key(f[3], "w=")?)?).map_err(|_| Error::Invalid)?;
        let height = u16::try_from(number(key(f[4], "h=")?)?).map_err(|_| Error::Invalid)?;
        if width == 0 || height == 0 {
            return Err(Error::Invalid);
        }
        let formats = key(f[5], "formats=")?;
        if !formats_valid(formats) {
            return Err(Error::Invalid);
        }
        let free_kib = number(key(f[6], "free_kib=")?)?;
        let battery = match key(f[7], "battery=")? {
            "na" => None,
            b => Some(
                u8::try_from(number(b)?)
                    .ok()
                    .filter(|b| *b <= 100)
                    .ok_or(Error::Invalid)?,
            ),
        };
        Ok(Self {
            firmware,
            width,
            height,
            formats,
            free_kib,
            battery,
        })
    }
    pub fn write(&self, out: &mut [u8]) -> Result<usize> {
        let mut battery = [0; 3];
        let b = match self.battery {
            None => "na",
            Some(b) => {
                let n = formatted(&mut battery, format_args!("{b}"))?;
                text(&battery[..n])?
            }
        };
        let n = formatted(
            out,
            format_args!(
                "hello 1 fw={} w={} h={} formats={} free_kib={} battery={}\n",
                self.firmware, self.width, self.height, self.formats, self.free_kib, b
            ),
        )?;
        Hello::parse(&out[..n])?;
        Ok(n)
    }
}
/// The payload formats a device can take (PROTOCOL.md C): `tk1` tokens,
/// `ar1` articles, `im1` pictures (ADR 028).
pub const FORMATS: [&str; 3] = ["tk1", "ar1", "im1"];

/// One to three known formats, comma-separated, each at most once.
fn formats_valid(list: &str) -> bool {
    let mut seen = [false; FORMATS.len()];
    let mut n = 0;
    for f in list.split(',') {
        let Some(i) = FORMATS.iter().position(|k| *k == f) else {
            return false;
        };
        if core::mem::replace(&mut seen[i], true) {
            return false;
        }
        n += 1;
    }
    n > 0
}

impl Hello<'_> {
    /// Whether the device takes this payload format.
    pub fn takes(&self, format: &str) -> bool {
        self.formats.split(',').any(|f| f == format)
    }
}

/// The payload format a job kind carries (`delete` carries none).
pub fn format_of(kind: &str) -> Option<&'static str> {
    match kind {
        "token" => Some("tk1"),
        "article" => Some("ar1"),
        "image" => Some("im1"),
        _ => None,
    }
}

pub fn parse_time(bytes: &[u8]) -> Result<u64> {
    let f = fields::<3>(single(bytes, LINE_CAP)?)?;
    if f[0] != "ok" || f[1] != "1" {
        return Err(Error::Invalid);
    }
    number(key(f[2], "time=")?)
}
pub fn write_time(time: u64, out: &mut [u8]) -> Result<usize> {
    formatted(out, format_args!("ok 1 time={time}\n"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Job<'a> {
    pub id: &'a str,
    pub kind: &'a str,
    pub item: &'a str,
    pub len: usize,
    pub sha256: Option<&'a str>,
    pub show: bool,
}
impl<'a> Job<'a> {
    fn parse(line: &'a str) -> Result<Self> {
        let f = fields::<7>(line)?;
        if f[0] != "job" {
            return Err(Error::Invalid);
        }
        let id = job_id(f[1])?;
        let kind = f[2];
        let item = item_id(f[3])?;
        let len = usize::try_from(number(f[4])?).map_err(|_| Error::TooLarge)?;
        let picture = matches!(kind, "token" | "image");
        if len > ARTICLE_CAP || (picture && len > TOKEN_CAP) {
            return Err(Error::TooLarge);
        }
        let sha256 = if f[5] == "-" { None } else { Some(hash(f[5])?) };
        let show = match f[6] {
            "-" => false,
            "show" if picture => true,
            _ => return Err(Error::Invalid),
        };
        if kind == "delete" {
            if len != 0 || sha256.is_some() {
                return Err(Error::Invalid);
            }
        } else if sha256.is_none() {
            return Err(Error::Invalid);
        }
        if !kind.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err(Error::Invalid);
        }
        Ok(Self {
            id,
            kind,
            item,
            len,
            sha256,
            show,
        })
    }
    pub fn supported(&self) -> bool {
        matches!(self.kind, "token" | "image" | "article" | "delete")
    }
    pub fn write(&self, out: &mut [u8]) -> Result<usize> {
        let n = formatted(
            out,
            format_args!(
                "job {} {} {} {} {} {}\n",
                self.id,
                self.kind,
                self.item,
                self.len,
                self.sha256.unwrap_or("-"),
                if self.show { "show" } else { "-" }
            ),
        )?;
        Job::parse(single(&out[..n], LINE_CAP)?)?;
        Ok(n)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Jobs<'a> {
    pub jobs: [Option<Job<'a>>; 16],
    pub more: bool,
}
impl<'a> Jobs<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut lines = validate_lines(bytes, JOBS_CAP)?.split_terminator('\n');
        if lines.next() != Some("x3jobs 1") {
            return Err(Error::Invalid);
        }
        let mut jobs = [None; 16];
        let mut count = 0;
        loop {
            let line = lines.next().ok_or(Error::Truncated)?;
            if matches!(line, "end more=0" | "end more=1") {
                if lines.next().is_some() {
                    return Err(Error::Invalid);
                }
                return Ok(Self {
                    jobs,
                    more: line == "end more=1",
                });
            }
            if count == 16 {
                return Err(Error::TooLarge);
            }
            let job = Job::parse(line)?;
            if jobs.iter().flatten().any(|j: &Job<'_>| j.id == job.id) {
                return Err(Error::Invalid);
            }
            jobs[count] = Some(job);
            count += 1;
        }
    }
    pub fn write(&self, out: &mut [u8]) -> Result<usize> {
        let mut n = formatted(out, format_args!("x3jobs 1\n"))?;
        for job in self.jobs.iter().flatten() {
            n += job.write(&mut out[n..])?;
        }
        n += formatted(
            &mut out[n..],
            format_args!("end more={}\n", u8::from(self.more)),
        )?;
        Jobs::parse(&out[..n])?;
        Ok(n)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ack<'a> {
    Committed(Option<&'a str>),
    Displayed(&'a str),
    Rejected(&'a str),
}
impl<'a> Ack<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let f = fields::<2>(single(bytes, LINE_CAP)?)?;
        match f[0] {
            "committed" => Ok(Self::Committed(if f[1] == "-" {
                None
            } else {
                Some(hash(f[1])?)
            })),
            "displayed" => Ok(Self::Displayed(hash(f[1])?)),
            "rejected"
                if matches!(
                    f[1],
                    "checksum" | "too_large" | "invalid" | "unsupported" | "storage"
                ) =>
            {
                Ok(Self::Rejected(f[1]))
            }
            _ => Err(Error::Invalid),
        }
    }
    pub fn write(&self, out: &mut [u8]) -> Result<usize> {
        let (a, b) = match self {
            Self::Committed(h) => ("committed", h.unwrap_or("-")),
            Self::Displayed(h) => ("displayed", *h),
            Self::Rejected(r) => ("rejected", *r),
        };
        let n = formatted(out, format_args!("{a} {b}\n"))?;
        Ack::parse(&out[..n])?;
        Ok(n)
    }
}
pub fn parse_error(bytes: &[u8]) -> Result<&str> {
    let f = fields::<2>(single(bytes, LINE_CAP)?)?;
    if f[0] != "error"
        || !matches!(
            f[1],
            "invalid" | "unauthorised" | "not_found" | "conflict" | "too_large" | "unavailable"
        )
    {
        return Err(Error::Invalid);
    }
    Ok(f[1])
}
pub fn write_error(code: &str, out: &mut [u8]) -> Result<usize> {
    let n = formatted(out, format_args!("error {code}\n"))?;
    parse_error(&out[..n])?;
    Ok(n)
}

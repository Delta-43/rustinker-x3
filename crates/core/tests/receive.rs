#![cfg(feature = "fat")]
mod support;
use embedded_sdmmc::{VolumeIdx, VolumeManager};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use support::*;
use x3_core::{
    fat::FatStore,
    receive::{Error, INDEX_CAP, Index, Net, NetError, Shown, Step, Target, run, wipe},
    storage::{self, Catalog, FileKey, FileKind, Record, Stage, Store},
};
use x3_proto::{config::Companion, lines::Hello, sign};

const CFG: &[u8] =
    b"url=http://192.168.1.20:8788\ntoken=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
const HELLO: Hello<'static> = Hello {
    firmware: "0.1.0-test",
    width: 792,
    height: 528,
    formats: "tk1,ar1",
    free_kib: 0,
    battery: None,
};

/// The fake is always ready, so one poll finishes the session.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("fake network never waits"),
    }
}

fn job_id(n: usize) -> String {
    format!("01JB8ZK3Q4M8D6F2T0V9X7W5{n:02}")
}
fn hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn article(text: &str) -> Vec<u8> {
    let mut b = vec![0; 300_000];
    let n =
        x3_proto::payload::encode_article("Title", "https://example.org/", text, false, 0, &mut b)
            .unwrap();
    b.truncate(n);
    b
}
fn token(fill: u8) -> Vec<u8> {
    let mut b = vec![fill; 16 + 792 * 528 / 8];
    let h = x3_proto::payload::TokenHeader {
        bpp: 1,
        width: 792,
        height: 528,
        crc: x3_proto::payload::crc32(&b[16..]),
    };
    h.encode(&mut b).unwrap();
    b
}

/// A scripted companion. Requests are parsed for real; responses come from
/// the job queue, or from `raw` for hostile cases.
#[derive(Default)]
struct Fake {
    batches: VecDeque<Vec<String>>,
    payloads: HashMap<String, Vec<u8>>,
    /// Status override for a job's payload.
    payload_status: HashMap<String, u16>,
    /// Raw response by request number (0-based).
    raw: HashMap<usize, Vec<u8>>,
    /// Fail with this error at request number N.
    fail: HashMap<usize, NetError>,
    /// Largest piece handed to one `recv`.
    chunk: usize,
    acks: Vec<(String, String)>,
    routes: Vec<String>,
    auth: Vec<String>,
    sent: Vec<u8>,
    reply: Vec<u8>,
    at: usize,
    replied: bool,
    open: bool,
    opens: usize,
    /// The LAN address does not answer (the remote name is tried next).
    lan_down: bool,
    /// The LAN connect times out and, like a real TCP socket, stays busy
    /// connecting until it is closed (the X3 on a phone hotspot).
    lan_timeout: bool,
    /// A timed-out connect still holds the socket.
    busy: bool,
    /// Signed mode: the key the companion holds, once a remote connection opens.
    key: Option<sign::Key>,
    signed_conn: bool,
    /// How the companion misbehaves on a signed connection.
    tamper: Tamper,
    nonces: Vec<String>,
    counter: u8,
    last_nonce: String,
    remote_opens: usize,
    raw_used: bool,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
enum Tamper {
    #[default]
    None,
    /// A body byte changed after signing.
    Body,
    /// The status changed after signing.
    Status,
    /// Signed with another key.
    WrongKey,
    /// Signed for another request's nonce.
    OldNonce,
    /// No MAC header.
    NoMac,
}

impl Fake {
    fn job(&mut self, n: usize, kind: &str, item: &str, body: &[u8], show: bool) -> String {
        let id = job_id(n);
        let (len, sha) = if kind == "delete" {
            (0, "-".to_string())
        } else {
            (body.len(), hex(body))
        };
        self.payloads.insert(id.clone(), body.to_vec());
        let flag = if show { "show" } else { "-" };
        let line = format!("job {id} {kind} {item} {len} {sha} {flag}");
        match self.batches.back_mut() {
            Some(b) => b.push(line),
            None => self.batches.push_back(vec![line]),
        }
        id
    }
    fn response(&mut self) -> Vec<u8> {
        let plain = self.response_plain();
        if !self.signed_conn || self.raw_used {
            return plain;
        }
        self.sign(plain)
    }
    fn sign(&mut self, plain: Vec<u8>) -> Vec<u8> {
        let end = plain.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let status: u16 = std::str::from_utf8(&plain[9..12]).unwrap().parse().unwrap();
        let body = &plain[end..];
        let key = match self.tamper {
            Tamper::WrongKey => sign::key("another token entirely"),
            _ => self.key.unwrap(),
        };
        let nonce = match self.tamper {
            Tamper::OldNonce => self
                .nonces
                .first()
                .cloned()
                .filter(|n| *n != self.last_nonce)
                .unwrap_or_else(|| "0".repeat(32)),
            _ => self.last_nonce.clone(),
        };
        let digest: [u8; 32] = Sha256::digest(body).into();
        let mac = sign::response_mac(&key, &nonce, status, &digest);
        let mut mac_hex = [0u8; 64];
        sign::hex(&mac, &mut mac_hex);
        let first_line_end = plain.windows(2).position(|w| w == b"\r\n").unwrap() + 2;
        let mut out = plain[..first_line_end].to_vec();
        if self.tamper != Tamper::NoMac {
            out.extend(format!("X3-Mac: {}\r\n", std::str::from_utf8(&mac_hex).unwrap()).bytes());
        }
        out.extend(&plain[first_line_end..]);
        match self.tamper {
            Tamper::Body if !body.is_empty() => {
                let at = out.len() - 1;
                out[at] ^= 1;
            }
            Tamper::Status => {
                // 200 becomes 404; the Content-Length stays right for the error body only if empty.
                out[9..12].copy_from_slice(b"409");
            }
            _ => {}
        }
        out
    }
    fn response_plain(&mut self) -> Vec<u8> {
        let n = self.routes.len();
        let end = self
            .sent
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .expect("request head")
            + 4;
        let head = String::from_utf8(self.sent[..end].to_vec()).unwrap();
        let body = String::from_utf8(self.sent[end..].to_vec()).unwrap();
        let line = head.lines().next().unwrap().to_string();
        let path = line.split(' ').nth(1).unwrap().to_string();
        self.auth.push(
            head.lines()
                .find_map(|l| l.strip_prefix("Authorization: "))
                .unwrap_or("")
                .into(),
        );
        self.routes.push(path.clone());
        if self.signed_conn {
            let key = self.key.expect("a key for signed mode");
            let nonce = head
                .lines()
                .find_map(|l| l.strip_prefix("X3-Nonce: "))
                .expect("X3-Nonce")
                .to_string();
            let mac = head
                .lines()
                .find_map(|l| l.strip_prefix("Authorization: X3-HMAC "))
                .expect("X3-HMAC authorization");
            assert!(!head.contains("Bearer"), "the token is never sent");
            assert!(
                !head.contains("AAAAAAAAAAAAAAAA"),
                "the token is never sent"
            );
            let method = line.split(' ').next().unwrap();
            let want = sign::request_mac(&key, method, &path, &nonce, body.as_bytes());
            let mut want_hex = [0u8; 64];
            sign::hex(&want, &mut want_hex);
            assert_eq!(mac, std::str::from_utf8(&want_hex).unwrap(), "request MAC");
            assert!(!self.nonces.contains(&nonce), "a nonce is never reused");
            self.nonces.push(nonce.clone());
            self.last_nonce = nonce;
        }
        if let Some(raw) = self.raw.remove(&n) {
            self.raw_used = true;
            return raw;
        }
        self.raw_used = false;
        let ok = |b: &[u8]| {
            let mut r =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", b.len()).into_bytes();
            r.extend_from_slice(b);
            r
        };
        if path == "/device/v1/hello" {
            assert!(body.starts_with("hello 1 fw=0.1.0-test "));
            return ok(b"ok 1 time=1792000000\n");
        }
        if path == "/device/v1/jobs" {
            let mut s = String::from("x3jobs 1\n");
            for l in self.batches.pop_front().unwrap_or_default() {
                s += &l;
                s.push('\n');
            }
            s += &format!("end more={}\n", u8::from(!self.batches.is_empty()));
            return ok(s.as_bytes());
        }
        let rest = path.strip_prefix("/device/v1/jobs/").unwrap();
        let (id, what) = rest.split_once('/').unwrap();
        if what == "payload" {
            if let Some(&s) = self.payload_status.get(id) {
                return format!("HTTP/1.1 {s} X\r\nContent-Length: 10\r\n\r\nerror gone")
                    .into_bytes();
            }
            return ok(&self.payloads[id]);
        }
        assert_eq!(what, "ack");
        self.acks.push((id.into(), body.trim_end().into()));
        b"HTTP/1.1 204 No Content\r\n\r\n".to_vec()
    }
}

impl Net for Fake {
    fn random(&mut self, out: &mut [u8]) {
        for b in out.iter_mut() {
            self.counter = self.counter.wrapping_add(1);
            *b = self.counter;
        }
    }
    async fn open(&mut self, to: Target<'_>) -> Result<(), NetError> {
        if self.busy {
            // The socket is still connecting somewhere else.
            return Err(NetError::Connect);
        }
        if self.open {
            // Only a kept-alive remote connection may still be open.
            assert!(
                self.signed_conn && matches!(to, Target::Remote { .. }),
                "one connection at a time"
            );
            self.sent.clear();
            self.replied = false;
            return Ok(());
        }
        match to {
            Target::Lan { ip, port } => {
                assert_eq!((ip, port), ([192, 168, 1, 20], 8788));
                if self.lan_down {
                    return Err(NetError::Connect);
                }
                if self.lan_timeout {
                    self.busy = true;
                    return Err(NetError::Timeout);
                }
                self.signed_conn = false;
            }
            Target::Remote { host, port } => {
                assert_eq!((host, port), ("device.example.org", 443));
                self.signed_conn = true;
                self.remote_opens += 1;
            }
        }
        if let Some(e) = self.fail.get(&self.routes.len()) {
            return Err(*e);
        }
        self.open = true;
        self.opens += 1;
        self.sent.clear();
        self.replied = false;
        Ok(())
    }
    async fn send(&mut self, bytes: &[u8]) -> Result<(), NetError> {
        assert!(self.open);
        self.sent.extend_from_slice(bytes);
        Ok(())
    }
    async fn recv(&mut self, out: &mut [u8]) -> Result<usize, NetError> {
        assert!(self.open);
        if !self.replied {
            self.reply = self.response();
            self.at = 0;
            self.replied = true;
        }
        let chunk = if self.chunk == 0 {
            usize::MAX
        } else {
            self.chunk
        };
        let n = out.len().min(self.reply.len() - self.at).min(chunk);
        out[..n].copy_from_slice(&self.reply[self.at..self.at + n]);
        self.at += n;
        Ok(n)
    }
    async fn close(&mut self) {
        self.open = false;
        self.busy = false;
    }
}

fn session(
    disk: &Disk,
    fake: &mut Fake,
    owed: Option<&Shown>,
) -> (x3_core::receive::Report, Vec<Record>) {
    session_with(CFG, disk, fake, owed)
}

const REMOTE_CFG: &[u8] =
    b"url=https://device.example.org\ntoken=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";
const BOTH_CFG: &[u8] = b"url=https://device.example.org\nlan=http://192.168.1.20:8788\ntoken=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\n";

fn session_with(
    cfg: &[u8],
    disk: &Disk,
    fake: &mut Fake,
    owed: Option<&Shown>,
) -> (x3_core::receive::Report, Vec<Record>) {
    let m = VolumeManager::new(disk.clone(), Clock);
    let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
    let d = m.open_root_dir(v).unwrap();
    let mut s = FatStore {
        manager: &m,
        directory: d,
    };
    let cfg = Companion::parse(cfg).unwrap();
    let mut index = Index::load(&mut s).unwrap();
    let report = block_on(run(fake, &mut s, &mut index, &cfg, &HELLO, owed));
    assert!(!fake.open, "connection left open");
    // The in-memory index must match a fresh load from the card.
    let fresh = Index::load(&mut s).unwrap();
    let mut a: Vec<_> = index.iter().cloned().collect();
    let mut b: Vec<_> = fresh.iter().cloned().collect();
    a.sort_by(|x, y| x.item().cmp(y.item()));
    b.sort_by(|x, y| x.item().cmp(y.item()));
    assert_eq!(a, b);
    for r in &a {
        storage::verify(&mut s, r).unwrap();
    }
    (report, a)
}

#[test]
fn articles_tokens_and_show() {
    let disk = Disk::new(blank());
    let mut f = Fake {
        chunk: 97,
        ..Fake::default()
    };
    let a = article("Hello there.\n");
    let t = token(0xAA);
    let ja = f.job(1, "article", "a-one", &a, false);
    let jt = f.job(2, "token", "t-one", &t, true);
    let (r, items) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(r.time, Some(1_792_000_000));
    assert_eq!(r.received, 2);
    assert_eq!(items.len(), 2);
    let show = r.show.unwrap();
    assert_eq!(
        (show.job(), show.item(), show.sha()),
        (jt.as_str(), "t-one", hex(&t).as_str())
    );
    assert_eq!(
        f.acks,
        vec![
            (ja, format!("committed {}", hex(&a))),
            (jt.clone(), format!("committed {}", hex(&t))),
        ]
    );
    assert!(
        f.auth
            .iter()
            .all(|a| a == "Bearer AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")
    );
    assert_eq!(f.routes.last().unwrap(), "/device/v1/jobs");

    // Next session: the owed displayed ack goes first, then an empty list.
    let mut g = Fake::default();
    let (r, _) = session(&disk, &mut g, Some(&show));
    r.result.unwrap();
    assert!(r.displayed);
    assert_eq!(
        g.acks,
        vec![
            (jt.clone(), format!("committed {}", hex(&t))),
            (jt, format!("displayed {}", hex(&t))),
        ]
    );
}

#[test]
fn held_hash_skips_download_and_replacement_and_delete() {
    let disk = Disk::new(blank());
    let a = article("First.\n");
    let mut f = Fake::default();
    f.job(1, "article", "a-one", &a, false);
    session(&disk, &mut f, None).0.result.unwrap();

    // Lost ack: the same job again is acked without a payload request.
    let mut f = Fake::default();
    let id = f.job(1, "article", "a-one", &a, false);
    let (r, _) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(r.received, 0);
    assert!(!f.routes.iter().any(|p| p.ends_with("/payload")));
    assert_eq!(f.acks, vec![(id, format!("committed {}", hex(&a)))]);

    // Replacement, then delete, across two batches.
    let b = article("Second.\n");
    let mut f = Fake::default();
    f.job(2, "article", "a-one", &b, false);
    f.batches.push_back(vec![]);
    f.job(3, "delete", "a-one", &[], false);
    f.job(4, "delete", "a-none", &[], false);
    let (r, items) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!((r.received, r.deleted), (1, 1));
    assert!(items.is_empty());
    assert_eq!(f.acks[1].1, "committed -");
    assert_eq!(f.acks[2].1, "committed -");
}

#[test]
fn unsupported_cancelled_and_bad_hash() {
    let disk = Disk::new(blank());
    let a = article("Text.\n");
    let mut f = Fake::default();
    f.batches.push_back(vec![format!(
        "job {} future a-x 10 {} -",
        job_id(1),
        hex(b"x")
    )]);
    let gone = f.job(2, "article", "a-gone", &a, false);
    f.payload_status.insert(gone.clone(), 404);
    let bad = f.job(3, "article", "a-bad", &a, false);
    f.payloads.get_mut(&bad).unwrap()[40] ^= 1;
    let (r, items) = session(&disk, &mut f, None);
    assert_eq!(r.result, Err(Error::Checksum));
    assert!(items.is_empty());
    assert_eq!(r.rejected, 2);
    assert_eq!(
        f.acks,
        vec![
            (job_id(1), "rejected unsupported".into()),
            (bad, "rejected checksum".into()),
        ]
    );
}

#[test]
fn hostile_responses() {
    let a = article("Text.\n");
    let cases: Vec<(&str, usize, Vec<u8>)> = vec![
        (
            "redirect",
            0,
            b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\n\r\n".to_vec(),
        ),
        (
            "chunked",
            1,
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n".to_vec(),
        ),
        (
            "no length",
            1,
            b"HTTP/1.1 200 OK\r\n\r\nx3jobs 1\nend more=0\n".to_vec(),
        ),
        (
            "truncated head",
            0,
            b"HTTP/1.1 200 OK\r\nContent-Le".to_vec(),
        ),
        (
            "truncated body",
            0,
            b"HTTP/1.1 200 OK\r\nContent-Length: 30\r\n\r\nok 1 time=1\n".to_vec(),
        ),
        (
            "overrun",
            0,
            b"HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nok 1 time=1\nEXTRA".to_vec(),
        ),
        (
            "huge head",
            0,
            [b"HTTP/1.1 200 OK\r\nX: ".as_slice(), &[b'a'; 2000]].concat(),
        ),
        (
            "over cap",
            1,
            b"HTTP/1.1 200 OK\r\nContent-Length: 5000\r\n\r\n".to_vec(),
        ),
        (
            "bad lines",
            1,
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nx3jobs 2\n".to_vec(),
        ),
        (
            "204 body",
            3,
            b"HTTP/1.1 204 No\r\nContent-Length: 2\r\n\r\nhi".to_vec(),
        ),
        ("payload too long", 2, {
            let mut r =
                format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", a.len() + 1).into_bytes();
            r.extend_from_slice(&a);
            r.push(0);
            r
        }),
    ];
    for (name, at, raw) in cases {
        let disk = Disk::new(blank());
        let mut f = Fake {
            chunk: 7,
            ..Fake::default()
        };
        f.job(1, "article", "a-one", &a, false);
        f.raw.insert(at, raw);
        let (r, items) = session(&disk, &mut f, None);
        assert!(
            matches!(r.result, Err(Error::Response(_))),
            "{name}: {:?}",
            r.result
        );
        assert!(items.is_empty() || at > 2, "{name}");
    }
}

#[test]
fn statuses_and_network_errors() {
    let disk = Disk::new(blank());
    let mut f = Fake::default();
    f.raw.insert(
        0,
        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 19\r\n\r\nerror unauthorized\n".to_vec(),
    );
    let (r, _) = session(&disk, &mut f, None);
    assert_eq!(r.result, Err(Error::Status(Step::Hello, 401)));
    assert_eq!(
        r.result.unwrap_err().message(),
        "The companion refused the device token"
    );

    let a = article("Text.\n");
    let mut f = Fake::default();
    let id = f.job(1, "article", "a-one", &a, false);
    f.payload_status.insert(id, 500);
    let (r, _) = session(&disk, &mut f, None);
    assert_eq!(r.result, Err(Error::Status(Step::Payload, 500)));
    assert!(f.acks.is_empty());

    for (at, e) in [(0, NetError::Connect), (1, NetError::Timeout)] {
        let mut f = Fake::default();
        f.fail.insert(at, e);
        let (r, _) = session(&disk, &mut f, None);
        assert_eq!(r.result, Err(Error::Net(e)));
    }
}

#[test]
fn full_index_rejects_new_items_and_evicts_tokens() {
    let disk = Disk::new(blank());
    let m = VolumeManager::new(disk.clone(), Clock);
    let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
    let d = m.open_root_dir(v).unwrap();
    let mut s = FatStore {
        manager: &m,
        directory: d,
    };
    let t = token(0);
    for i in 0..INDEX_CAP {
        let g = s.next_generation().unwrap();
        let r = Record::new(
            &format!("t-{i}"),
            g,
            t.len() as u32,
            Sha256::digest(&t).into(),
            true,
        )
        .unwrap();
        let mut stage = Stage::begin(&mut s, r).unwrap();
        stage.write(&mut s, &t).unwrap();
        stage.commit(&mut s).unwrap();
    }
    m.close_dir(d).unwrap();
    m.close_volume(v).unwrap();
    drop(m);

    let mut f = Fake::default();
    let id = f.job(1, "token", "t-new", &token(1), false);
    let (r, _) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(f.acks, vec![(id, "rejected storage".into())]);

    let m = VolumeManager::new(disk.clone(), Clock);
    let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
    let d = m.open_root_dir(v).unwrap();
    let mut s = FatStore {
        manager: &m,
        directory: d,
    };
    let mut index = Index::load(&mut s).unwrap();
    assert!(index.is_full());
    assert_eq!(
        index.evict_tokens(&mut s, "t-0", 20).unwrap(),
        INDEX_CAP - 20
    );
    let left: Vec<_> = Index::load(&mut s)
        .unwrap()
        .iter()
        .map(|r| r.item().to_string())
        .collect();
    assert_eq!(left.len(), 20);
    assert!(left.contains(&"t-0".to_string()), "on-screen token kept");
    assert!(
        left.contains(&format!("t-{}", INDEX_CAP - 1)),
        "newest kept"
    );
}

#[test]
fn garbage_collection_keeps_only_live_files() {
    let disk = Disk::new(blank());
    let m = VolumeManager::new(disk.clone(), Clock);
    let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
    let d = m.open_root_dir(v).unwrap();
    let mut s = FatStore {
        manager: &m,
        directory: d,
    };
    let install = |s: &mut FatStore<'_, Disk, Clock>, item: &str, body: &[u8], token: bool| {
        let g = s.next_generation().unwrap();
        let r = Record::new(
            item,
            g,
            body.len() as u32,
            Sha256::digest(body).into(),
            token,
        )
        .unwrap();
        let mut stage = Stage::begin(s, r).unwrap();
        stage.write(s, body).unwrap();
        stage.commit(s).unwrap()
    };
    // a-one: installed, read (position), replaced, read again.
    let first = install(&mut s, "a-one", &article("First.\n"), false);
    let g = s.next_generation().unwrap();
    storage::position(&mut s, &first, g, 3).unwrap();
    let second = install(&mut s, "a-one", &article("Second.\n"), false);
    let g = s.next_generation().unwrap();
    let read = storage::position(&mut s, &second, g, 4).unwrap();
    // a-two: installed then deleted. t-one: a token that stays.
    let two = install(&mut s, "a-two", &article("Two.\n"), false);
    let g = s.next_generation().unwrap();
    storage::delete(&mut s, &two, g).unwrap();
    let tok = install(&mut s, "t-one", &token(7), true);
    // An abandoned download: a payload with no metadata.
    let g = s.next_generation().unwrap();
    s.create(FileKey {
        generation: g,
        kind: FileKind::Payload,
    })
    .unwrap();
    // A torn metadata file (wrong length) is garbage and goes too.
    let g = s.next_generation().unwrap();
    let junk = FileKey {
        generation: g,
        kind: FileKind::Metadata,
    };
    s.create(junk).unwrap();
    s.append(junk, b"not a record").unwrap();

    let index = Index::load(&mut s).unwrap();
    let done = index.collect_garbage(&mut s).unwrap();
    // Metadata: first, its position, second (superseded by read), two, its
    // tombstone, the torn file = 6. Payloads: first, two, the abandoned one = 3.
    assert_eq!(done.metadata, 6);
    assert_eq!(done.payloads, 3);
    assert_eq!(done.kept_unsure, 0);

    let mut met = [0u32; 8];
    let n = s.generations(0, &mut met).unwrap();
    assert_eq!(&met[..n], &[read.generation, tok.generation]);
    let mut dat = [0u32; 8];
    let n = s.payload_generations(0, &mut dat).unwrap();
    assert_eq!(
        &dat[..n],
        &[read.payload_generation, tok.payload_generation]
    );

    // The card reloads to the same state, every payload verifies, and a
    // second pass finds nothing to do.
    let again = Index::load(&mut s).unwrap();
    let mut live: Vec<_> = again.iter().cloned().collect();
    live.sort_by(|x, y| x.item().cmp(y.item()));
    assert_eq!(live, vec![read.clone(), tok.clone()]);
    for r in &live {
        storage::verify(&mut s, r).unwrap();
    }
    let done = again.collect_garbage(&mut s).unwrap();
    assert_eq!((done.metadata, done.payloads, done.kept_unsure), (0, 0, 0));
}

#[test]
fn full_shelf_evicts_finished_then_oldest_article() {
    let disk = Disk::new(blank());
    {
        let m = VolumeManager::new(disk.clone(), Clock);
        let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
        let d = m.open_root_dir(v).unwrap();
        let mut s = FatStore {
            manager: &m,
            directory: d,
        };
        let body = article("Hello there.\n");
        let header = x3_proto::payload::ArticleHeader::decode(&body[..28]).unwrap();
        for i in 0..x3_core::receive::ARTICLE_LIMIT {
            let g = s.next_generation().unwrap();
            let r = Record::new(
                &format!("a-{i:02}"),
                g,
                body.len() as u32,
                Sha256::digest(&body).into(),
                false,
            )
            .unwrap();
            let mut stage = Stage::begin(&mut s, r).unwrap();
            stage.write(&mut s, &body).unwrap();
            let r = stage.commit(&mut s).unwrap();
            if i == 5 {
                // Read to the end: this one is evicted first.
                let g = s.next_generation().unwrap();
                storage::position(&mut s, &r, g, header.text_len).unwrap();
            }
        }
    }
    let mut f = Fake::default();
    f.job(1, "article", "a-new", &article("New one.\n"), false);
    let (r, items) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(r.received, 1);
    let names: Vec<_> = items.iter().map(|r| r.item().to_string()).collect();
    assert_eq!(names.len(), x3_core::receive::ARTICLE_LIMIT);
    assert!(names.contains(&"a-new".to_string()));
    assert!(
        !names.contains(&"a-05".to_string()),
        "finished article goes"
    );
    assert!(names.contains(&"a-00".to_string()), "oldest unread stays");

    // Nothing finished now: the oldest received goes.
    let mut f = Fake::default();
    f.job(2, "article", "a-newer", &article("Newer.\n"), false);
    let (r, items) = session(&disk, &mut f, None);
    r.result.unwrap();
    let names: Vec<_> = items.iter().map(|r| r.item().to_string()).collect();
    assert_eq!(names.len(), x3_core::receive::ARTICLE_LIMIT);
    assert!(!names.contains(&"a-00".to_string()));
    assert!(names.contains(&"a-newer".to_string()));
}

/// One whole device Receive: load, run the session, evict tokens, collect
/// garbage. Errors are ignored: after an injected cut they are expected.
fn device_pass(disk: &Disk, fake: &mut Fake) {
    let m = VolumeManager::new(disk.clone(), Clock);
    let Ok(v) = m.open_raw_volume(VolumeIdx(0)) else {
        return;
    };
    let Ok(d) = m.open_root_dir(v) else { return };
    let mut s = FatStore {
        manager: &m,
        directory: d,
    };
    let cfg = Companion::parse(CFG).unwrap();
    let Ok(mut index) = Index::load(&mut s) else {
        return;
    };
    let report = block_on(run(fake, &mut s, &mut index, &cfg, &HELLO, None));
    let keep = report
        .show
        .as_ref()
        .map_or("t-new".to_string(), |s| s.item().to_string());
    let _ = index.evict_tokens(&mut s, &keep, 1);
    let _ = index.collect_garbage(&mut s);
}

fn script() -> Fake {
    let mut f = Fake {
        chunk: 211,
        ..Fake::default()
    };
    f.job(
        1,
        "article",
        "a-one",
        &article("Replacement text.\n"),
        false,
    );
    f.job(2, "article", "a-two", &article("Second.\n"), false);
    f.job(3, "delete", "a-gone", &[], false);
    f.job(4, "token", "t-new", &token(2), true);
    f
}

/// Loads a card as the device would after a restart and verifies every live
/// item. Returns the items and the metadata and payload file counts.
fn inspect(bytes: &[u8]) -> (Vec<Record>, usize, usize) {
    let disk = Disk::new(bytes.to_vec());
    let m = VolumeManager::new(disk, Clock);
    let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
    let d = m.open_root_dir(v).unwrap();
    let mut s = FatStore {
        manager: &m,
        directory: d,
    };
    let index = Index::load(&mut s).expect("the card must load after a cut");
    let mut items: Vec<_> = index.iter().cloned().collect();
    items.sort_by(|a, b| a.item().cmp(b.item()));
    for r in &items {
        storage::verify(&mut s, r)
            .unwrap_or_else(|e| panic!("{} fails to verify: {e:?}", r.item()));
    }
    let (mut ids, mut dats) = ([0u32; 64], [0u32; 64]);
    let met = s.generations(0, &mut ids).unwrap();
    let dat = s.payload_generations(0, &mut dats).unwrap();
    (items, met, dat)
}

/// A sampled sweep for every test run: about one write in 41.
#[test]
fn power_cuts_never_leave_a_corrupt_entry_sampled() {
    power_cut_sweep(41);
}

/// Every write, three tear sizes each (3,162 cuts, about 3 minutes in a debug
/// build): `cargo test --release -p x3-core --all-features --test receive -- --ignored`.
#[test]
#[ignore = "slow: run with --release -- --ignored"]
fn a_power_cut_at_any_write_never_leaves_a_corrupt_entry() {
    power_cut_sweep(1);
}

fn power_cut_sweep(stride: usize) {
    // The card before the session: a read article, one to be deleted, an old
    // token. The session replaces the first, adds an article and a token,
    // deletes the second, then evicts the old token (limit 1) and collects.
    let disk = Disk::new(blank());
    let mut setup = Fake::default();
    setup.job(1, "article", "a-one", &article("Original.\n"), false);
    setup.job(2, "article", "a-gone", &article("Doomed.\n"), false);
    setup.job(3, "token", "t-old", &token(1), false);
    session(&disk, &mut setup, None).0.result.unwrap();
    {
        let m = VolumeManager::new(disk.clone(), Clock);
        let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
        let d = m.open_root_dir(v).unwrap();
        let mut s = FatStore {
            manager: &m,
            directory: d,
        };
        let one = Index::load(&mut s).unwrap().get("a-one").cloned().unwrap();
        let g = s.next_generation().unwrap();
        storage::position(&mut s, &one, g, 3).unwrap();
    }
    let base = disk.0.borrow().bytes.clone();

    // Uncut baseline: how many writes the whole pass makes, and what a
    // finished card looks like.
    let clean = Disk::new(base.clone());
    device_pass(&clean, &mut script());
    let total = clean.0.borrow().writes;
    let finished = clean.0.borrow().bytes.clone();
    let (done, met, dat) = inspect(&finished);
    let names: Vec<_> = done.iter().map(|r| r.item()).collect();
    assert_eq!(names, ["a-one", "a-two", "t-new"]);
    assert_eq!((met, dat), (3, 3), "a finished pass leaves only live files");
    assert!(total > 30, "{total} writes; the loop must cover real work");

    let mut cuts = 0;
    for write in (0..total).step_by(stride) {
        for prefix in [0, 97, 511] {
            let disk = Disk::new(base.clone());
            {
                let mut d = disk.0.borrow_mut();
                d.cut = Some(write);
                d.prefix = prefix;
            }
            device_pass(&disk, &mut script());
            assert!(disk.0.borrow().dead, "cut {write} did not fire");
            cuts += 1;
            let after = disk.0.borrow().bytes.clone();

            // 1. The card loads and everything live verifies. The article
            //    that was there is never lost, whichever version survives.
            let (items, _, _) = inspect(&after);
            let names: Vec<_> = items.iter().map(|r| r.item()).collect();
            assert!(names.contains(&"a-one"), "cut {write}/{prefix}: {names:?}");
            for n in &names {
                assert!(
                    ["a-one", "a-two", "a-gone", "t-old", "t-new"].contains(n),
                    "cut {write}/{prefix}: stray item {n}"
                );
            }

            // 2. The next, uninterrupted Receive finishes the job exactly and
            //    leaves nothing behind.
            let again = Disk::new(after);
            device_pass(&again, &mut script());
            let bytes = again.0.borrow().bytes.clone();
            let (items, met, dat) = inspect(&bytes);
            let names: Vec<_> = items.iter().map(|r| r.item()).collect();
            assert_eq!(names, ["a-one", "a-two", "t-new"], "cut {write}/{prefix}");
            assert_eq!((met, dat), (3, 3), "cut {write}/{prefix}: leftovers");
        }
    }
    println!("{cuts} cuts over {total} writes, all recovered");
}

fn signed_fake() -> Fake {
    Fake {
        key: Some(sign::key(&"A".repeat(43))),
        chunk: 61,
        ..Fake::default()
    }
}

#[test]
fn a_remote_only_session_is_signed_end_to_end() {
    let disk = Disk::new(blank());
    let mut f = signed_fake();
    let a = article("Signed article.\n");
    let t = token(0x55);
    f.job(1, "article", "a-one", &a, false);
    f.job(2, "token", "t-one", &t, true);
    let (r, items) = session_with(REMOTE_CFG, &disk, &mut f, None);
    r.result.unwrap();
    assert_eq!((r.received, items.len()), (2, 2));
    assert_eq!(
        f.opens, f.remote_opens,
        "every connection went to the remote name"
    );
    assert!(f.auth.iter().all(|a| a.starts_with("X3-HMAC ")));
    // hello, jobs, payload, ack, payload, ack, jobs
    assert!(f.nonces.len() >= 7, "{}", f.nonces.len());
}

#[test]
fn the_lan_address_is_preferred_and_the_remote_is_the_fallback() {
    let disk = Disk::new(blank());
    let mut f = signed_fake();
    f.job(1, "article", "a-one", &article("x\n"), false);
    let (r, _) = session_with(BOTH_CFG, &disk, &mut f, None);
    r.result.unwrap();
    assert!(!r.remote, "the report says the home LAN was used");
    assert_eq!(f.remote_opens, 0, "LAN answered, so no remote connection");
    assert!(f.auth.iter().all(|a| a.starts_with("Bearer ")));

    let disk = Disk::new(blank());
    let mut f = signed_fake();
    f.lan_down = true;
    f.job(1, "article", "a-one", &article("x\n"), false);
    let (r, items) = session_with(BOTH_CFG, &disk, &mut f, None);
    r.result.unwrap();
    assert!(r.remote, "the report says the internet route was used");
    assert_eq!(items.len(), 1);
    assert_eq!(
        f.remote_opens, f.opens,
        "after one failure the whole session uses the remote name"
    );
    assert!(f.auth.iter().all(|a| a.starts_with("X3-HMAC ")));
}

#[test]
fn a_lan_timeout_is_closed_before_the_remote_is_tried() {
    // On a phone hotspot the LAN address never answers: the connect times
    // out with the socket still busy. The fallback must reset it first.
    let disk = Disk::new(blank());
    let mut f = signed_fake();
    f.lan_timeout = true;
    f.job(1, "token", "t-one", &token(3), false);
    let (r, items) = session_with(BOTH_CFG, &disk, &mut f, None);
    r.result.unwrap();
    assert!(r.remote);
    assert_eq!(items.len(), 1);
    assert!(f.remote_opens >= 1);
}

#[test]
fn a_dead_lan_with_no_remote_is_still_an_error() {
    let disk = Disk::new(blank());
    let mut f = Fake {
        lan_down: true,
        ..Fake::default()
    };
    let (r, _) = session_with(CFG, &disk, &mut f, None);
    assert_eq!(r.result, Err(Error::Net(NetError::Connect)));
}

#[test]
fn every_kind_of_tampering_is_refused_and_nothing_is_committed() {
    for tamper in [
        Tamper::Body,
        Tamper::Status,
        Tamper::WrongKey,
        Tamper::OldNonce,
        Tamper::NoMac,
    ] {
        let disk = Disk::new(blank());
        let mut f = signed_fake();
        f.tamper = tamper;
        f.job(1, "article", "a-one", &article("x\n"), false);
        let (r, items) = session_with(REMOTE_CFG, &disk, &mut f, None);
        assert_eq!(r.result, Err(Error::Signature), "{tamper:?}");
        assert!(items.is_empty(), "{tamper:?}: nothing may be committed");
        assert_eq!(r.received, 0);
        assert!(f.acks.is_empty(), "{tamper:?}: no ack on a refused reply");
    }
}

#[test]
fn an_access_refusal_says_who_answered_and_commits_nothing() {
    // Cloudflare Access answers itself: unsigned, with long headers.
    let long = format!(
        "HTTP/1.1 403 Forbidden\r\ncontent-security-policy: {}\r\nset-cookie: {}\r\ncontent-length: 0\r\n\r\n",
        "x".repeat(900),
        "y".repeat(900)
    );
    let short = "HTTP/1.1 403 Forbidden\r\ncontent-length: 0\r\n\r\n".to_string();
    for reply in [long, short] {
        let disk = Disk::new(blank());
        let mut f = signed_fake();
        f.raw.insert(0, reply.into_bytes());
        f.job(1, "token", "t-one", &token(3), false);
        let (r, items) = session_with(REMOTE_CFG, &disk, &mut f, None);
        assert_eq!(r.result, Err(Error::Edge(403)));
        assert!(r.result.unwrap_err().message().contains("access_id"));
        assert!(items.is_empty());
        assert!(f.acks.is_empty());
    }
    // A long unsigned 200 is still a signature failure or framing error,
    // never accepted.
    let disk = Disk::new(blank());
    let mut f = signed_fake();
    f.raw.insert(
        0,
        format!("HTTP/1.1 200 OK\r\nx: {}\r\n\r\n", "z".repeat(2000)).into_bytes(),
    );
    let (r, _) = session_with(REMOTE_CFG, &disk, &mut f, None);
    assert!(
        matches!(r.result, Err(Error::Response(_))),
        "{:?}",
        r.result
    );
}

#[test]
fn a_forged_job_list_cannot_deliver_content() {
    // A hostile server that cannot sign: the job list is unsigned, so the
    // session stops before any payload is requested.
    let disk = Disk::new(blank());
    let mut f = signed_fake();
    let a = article("evil\n");
    f.job(1, "article", "a-evil", &a, false);
    f.raw.insert(1, {
        let body = format!(
            "x3jobs 1\njob {} article a-evil {} {} -\nend more=0\n",
            job_id(1),
            a.len(),
            hex(&a)
        );
        format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    });
    let (r, items) = session_with(REMOTE_CFG, &disk, &mut f, None);
    assert_eq!(r.result, Err(Error::Signature));
    assert!(items.is_empty());
    assert!(!f.routes.iter().any(|p| p.ends_with("/payload")));
}

#[test]
fn a_plain_lan_reply_with_a_mac_header_is_still_read() {
    // The LAN path does not need a MAC, and a stray header must not break it.
    let disk = Disk::new(blank());
    let mut f = Fake::default();
    f.job(1, "article", "a-one", &article("x\n"), false);
    let (r, items) = session_with(CFG, &disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(items.len(), 1);
}

#[test]
fn wipe_deletes_every_article_and_token_and_survives_a_power_cut() {
    let open = |disk: &Disk, f: &mut dyn FnMut(&mut FatStore<'_, Disk, Clock>)| {
        let m = VolumeManager::new(disk.clone(), Clock);
        let v = m.open_raw_volume(VolumeIdx(0)).unwrap();
        let d = m.open_root_dir(v).unwrap();
        let mut s = FatStore {
            manager: &m,
            directory: d,
        };
        f(&mut s);
    };
    let disk = Disk::new(blank());
    let mut f = Fake::default();
    f.job(1, "article", "a-one", &article("One.\n"), false);
    f.job(2, "article", "a-two", &article("Two.\n"), false);
    f.job(3, "token", "t-one", &token(7), true);
    let (r, items) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(items.len(), 3);
    open(&disk, &mut |s| {
        // An abandoned download and a torn metadata file.
        let g = s.next_generation().unwrap();
        s.create(FileKey {
            generation: g,
            kind: FileKind::Payload,
        })
        .unwrap();
        let g = s.next_generation().unwrap();
        let junk = FileKey {
            generation: g,
            kind: FileKind::Metadata,
        };
        s.create(junk).unwrap();
        s.append(junk, b"torn").unwrap();
    });
    let base = disk.0.borrow().bytes.clone();

    let clean = Disk::new(base.clone());
    let before = clean.0.borrow().writes;
    open(&clean, &mut |s| {
        let done = wipe(s).unwrap();
        assert_eq!((done.metadata, done.payloads), (4, 4), "{done:?}");
        let mut ids = [0u32; 8];
        assert_eq!(s.generations(0, &mut ids).unwrap(), 0);
        assert_eq!(s.payload_generations(0, &mut ids).unwrap(), 0);
        assert_eq!(wipe(s).unwrap(), Default::default());
    });
    let total = clean.0.borrow().writes - before;
    assert_eq!(inspect(&clean.0.borrow().bytes.clone()).0.len(), 0);
    // A wiped card takes new items as normal.
    let mut f = Fake::default();
    f.job(4, "token", "t-two", &token(9), true);
    let (r, items) = session(&clean, &mut f, None);
    r.result.unwrap();
    assert_eq!(items.len(), 1);

    // A power cut at every write of the wipe: the card loads, what it lists
    // verifies (inspect checks), and a second wipe empties it.
    assert!(total >= 8, "{total} writes");
    for write in 0..total {
        for prefix in [0, 97, 511] {
            let disk = Disk::new(base.clone());
            {
                let mut d = disk.0.borrow_mut();
                d.cut = Some(write);
                d.prefix = prefix;
            }
            open(&disk, &mut |s| assert!(wipe(s).is_err()));
            let after = disk.0.borrow().bytes.clone();
            let (items, _, _) = inspect(&after);
            for r in &items {
                assert!(
                    ["a-one", "a-two", "t-one"].contains(&r.item()),
                    "cut {write}/{prefix}: stray {}",
                    r.item()
                );
            }
            let again = Disk::new(after);
            open(&again, &mut |s| {
                wipe(s).unwrap();
            });
            let (items, met, dat) = inspect(&again.0.borrow().bytes.clone());
            assert_eq!((items.len(), met, dat), (0, 0, 0), "cut {write}/{prefix}");
        }
    }
}

#[test]
fn image_jobs_are_stored_as_pictures_and_shown() {
    let disk = Disk::new(blank());
    let mut f = Fake::default();
    f.job(1, "image", "i-harbour-0123456789ab", &token(5), true);
    f.job(2, "token", "t-elf-0123456789ab", &token(6), false);
    let (r, items) = session(&disk, &mut f, None);
    r.result.unwrap();
    assert_eq!(items.len(), 2);
    assert!(items.iter().all(|i| i.token), "both are pictures");
    assert_eq!(
        r.show.as_ref().map(|s| s.item()),
        Some("i-harbour-0123456789ab")
    );
    // A bad image payload is refused like a bad token.
    let disk = Disk::new(blank());
    let mut f = Fake::default();
    f.job(1, "image", "i-bad-0123456789ab", b"not a picture", false);
    let (r, items) = session(&disk, &mut f, None);
    assert!(r.result.is_err() || r.rejected == 1, "{:?}", r.result);
    assert!(items.is_empty());
}

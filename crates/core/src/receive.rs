//! One Receive session (PROTOCOL.md A.2), shared by the simulator and the
//! firmware. The caller supplies the network (`Net`) and the card
//! (`Catalog`); this module does the HTTP framing, the job loop and the acks.
//! Bounded: no allocation, fixed buffers, at most `INDEX_CAP` items.
use crate::{
    Error as StoreError,
    storage::{self, Catalog, FileKey, FileKind, RECORD_LEN, Record, Stage},
};
use sha2::{Digest, Sha256};
use x3_proto::{
    self as proto, JOBS_CAP,
    config::Companion,
    http::{Body, HEAD_CAP, ResponseHead, Route, nonce_hex},
    lines::{Ack, Hello, Jobs},
    sign,
};

/// Articles kept on the card; the 33rd evicts one (ADR 023).
pub const ARTICLE_LIMIT: usize = 32;

/// Live items the session can track: 20 tokens, 32 articles and slack.
pub const INDEX_CAP: usize = 64;
/// Largest error body read from the companion (one ack line plus LF).
const ERROR_BODY_CAP: usize = 193;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetError {
    Connect,
    Timeout,
    Io,
}

/// Where a connection goes. A LAN address is plain HTTP and the request
/// carries the device token; a remote name is TLS to an unchecked certificate
/// and the request is signed instead (ADR 026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    Lan { ip: [u8; 4], port: u16 },
    Remote { host: &'a str, port: u16 },
}

/// One TCP connection at a time. Each call must give up after the protocol
/// timeouts (connect 5 s, idle read 10 s) or when the whole session's
/// deadline has passed, and then return `NetError::Timeout`.
#[allow(async_fn_in_trait)]
pub trait Net {
    /// A remote target may already be open from the last request (signed
    /// requests keep the connection); then this returns at once.
    async fn open(&mut self, to: Target<'_>) -> Result<(), NetError>;
    /// Fills `out` with bytes from a random source. Nonces come from here.
    fn random(&mut self, out: &mut [u8]);
    async fn send(&mut self, bytes: &[u8]) -> Result<(), NetError>;
    /// `Ok(0)` means the companion closed the connection.
    async fn recv(&mut self, out: &mut [u8]) -> Result<usize, NetError>;
    async fn close(&mut self);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Hello,
    Jobs,
    Payload,
    Ack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Net(NetError),
    /// The response broke the framing or line rules.
    Response(proto::Error),
    /// An unexpected HTTP status.
    Status(Step, u16),
    Storage(StoreError),
    /// A payload failed its hash, header or CRC check.
    Checksum,
    /// A signed reply carried no MAC, or a wrong one (ADR 026).
    Signature,
    /// On the internet route, an unsigned reply with this HTTP status: the
    /// edge (Cloudflare Access, the proxy) answered, not the companion. Only
    /// a diagnosis: nothing in the reply is trusted or used.
    Edge(u16),
}

impl Error {
    /// A short sentence for the screen and the serial log. Never holds secrets.
    pub fn message(&self) -> &'static str {
        match self {
            Self::Net(NetError::Connect) => "Cannot reach the companion",
            Self::Net(NetError::Timeout) => "Receive timed out",
            Self::Net(NetError::Io) => "Connection lost",
            Self::Response(_) => "Bad response from the companion",
            Self::Status(_, 401) => "The companion refused the device token",
            Self::Status(..) => "The companion returned an error",
            Self::Storage(_) => "Cannot write to the card",
            Self::Checksum => "A download was damaged",
            Self::Signature => "The reply was not signed by your companion",
            Self::Edge(401 | 403) => {
                "The internet route refused the request: check access_id and access_secret in SERVER.CFG, and the Access policy"
            }
            Self::Edge(_) => "The internet route answered, not the companion",
        }
    }
}

impl From<NetError> for Error {
    fn from(e: NetError) -> Self {
        Self::Net(e)
    }
}
impl From<proto::Error> for Error {
    fn from(e: proto::Error) -> Self {
        Self::Response(e)
    }
}
impl From<StoreError> for Error {
    fn from(e: StoreError) -> Self {
        Self::Storage(e)
    }
}

/// Latest live record per item. Built once per session from the metadata
/// files, oldest generation first, so a tombstone simply removes its item.
/// Payloads are not re-verified here; a held payload is verified only before
/// a download is skipped because of it.
pub struct Index {
    records: [Option<Record>; INDEX_CAP],
    /// Articles evicted to make room since this index was loaded.
    pub articles_evicted: usize,
}

impl Default for Index {
    fn default() -> Self {
        Self {
            records: [const { None }; INDEX_CAP],
            articles_evicted: 0,
        }
    }
}

impl Index {
    /// `Err(TooLarge)` if the card holds more than `INDEX_CAP` live items.
    pub fn load(s: &mut impl Catalog) -> Result<Self, StoreError> {
        let mut index = Self::default();
        let mut after = 0;
        loop {
            let mut ids = [0u32; 16];
            let n = s.generations(after, &mut ids)?;
            if n == 0 {
                return Ok(index);
            }
            for &g in &ids[..n] {
                let key = FileKey {
                    generation: g,
                    kind: FileKind::Metadata,
                };
                let mut b = [0u8; RECORD_LEN];
                if s.len(key) == Ok(RECORD_LEN)
                    && s.read(key, 0, &mut b) == Ok(RECORD_LEN)
                    && let Ok(r) = Record::decode(&b)
                    && r.generation == g
                {
                    index.apply(r)?;
                }
            }
            after = ids[n - 1];
        }
    }

    /// Records a newer state of an item: a tombstone removes it.
    pub fn apply(&mut self, r: Record) -> Result<(), StoreError> {
        let slot = self.records.iter_mut().find(|s| match s {
            Some(old) => old.item() == r.item(),
            None => false,
        });
        match slot {
            Some(slot) if r.deleted => *slot = None,
            Some(slot) => *slot = Some(r),
            None if r.deleted => {}
            None => *self.free().ok_or(StoreError::TooLarge)? = Some(r),
        }
        Ok(())
    }

    fn free(&mut self) -> Option<&mut Option<Record>> {
        self.records.iter_mut().find(|s| s.is_none())
    }

    pub fn is_full(&self) -> bool {
        self.records.iter().all(Option::is_some)
    }

    pub fn get(&self, item: &str) -> Option<&Record> {
        self.records.iter().flatten().find(|r| r.item() == item)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.records.iter().flatten()
    }

    /// Makes room for one new article: when the card holds `ARTICLE_LIMIT`
    /// articles (or the index is full), deletes the oldest finished article,
    /// else the oldest received. Returns false if there is nothing to evict.
    pub fn make_room_for_article(&mut self, s: &mut impl Catalog) -> Result<bool, StoreError> {
        let articles = self.iter().filter(|r| !r.token).count();
        if articles < ARTICLE_LIMIT && !self.is_full() {
            return Ok(true);
        }
        let mut oldest_done: Option<Record> = None;
        let mut oldest: Option<Record> = None;
        for r in self.iter().filter(|r| !r.token) {
            if oldest
                .as_ref()
                .is_none_or(|o| r.payload_generation < o.payload_generation)
            {
                oldest = Some(r.clone());
            }
            if oldest_done
                .as_ref()
                .is_none_or(|o| r.payload_generation < o.payload_generation)
                && storage::finished(s, r).unwrap_or(false)
            {
                oldest_done = Some(r.clone());
            }
        }
        let Some(victim) = oldest_done.or(oldest) else {
            return Ok(false);
        };
        let generation = s.next_generation()?;
        let gone = storage::delete(s, &victim, generation)?;
        self.apply(gone)?;
        self.articles_evicted += 1;
        Ok(true)
    }

    /// Deletes the oldest tokens beyond `limit`, never the one on screen.
    /// Returns how many were deleted.
    pub fn evict_tokens(
        &mut self,
        s: &mut impl Catalog,
        on_screen: &str,
        limit: usize,
    ) -> Result<usize, StoreError> {
        if limit == 0 {
            return Err(StoreError::Invalid);
        }
        let mut n = 0;
        while self.iter().filter(|r| r.token).count() > limit {
            let victim = self
                .iter()
                .filter(|r| r.token && r.item() != on_screen)
                .min_by_key(|r| r.payload_generation)
                .cloned()
                .ok_or(StoreError::Storage)?;
            let generation = s.next_generation()?;
            let deleted = storage::delete(s, &victim, generation)?;
            self.apply(deleted)?;
            n += 1;
        }
        Ok(n)
    }
}

/// What one `Index::collect_garbage` pass removed or left alone.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Collected {
    pub metadata: usize,
    pub payloads: usize,
    /// Metadata files left in place: a read error, or newer than the index's
    /// record. Never removed on a guess.
    pub kept_unsure: usize,
}

impl Index {
    /// Removes superseded and abandoned files: metadata generations older than
    /// the item's live record, tombstones and everything under them, and
    /// payloads no live record points at.
    ///
    /// `self` must be current for the card and no download may be in flight.
    /// Metadata goes oldest first, so an item's tombstone is always its last
    /// file to go and a power cut cannot bring a deleted item back. Payloads
    /// go after metadata; a payload orphaned by a cut is taken next pass.
    pub fn collect_garbage(&self, s: &mut impl Catalog) -> Result<Collected, StoreError> {
        let mut done = Collected::default();
        let mut after = 0;
        loop {
            let mut ids = [0u32; 16];
            let n = s.generations(after, &mut ids)?;
            if n == 0 {
                break;
            }
            for &g in &ids[..n] {
                let key = FileKey {
                    generation: g,
                    kind: FileKind::Metadata,
                };
                // A wrong length, or a body that will not decode after a
                // successful read, is a torn write: garbage, safe to remove
                // (the index already ignores it). Only a read error is left
                // alone, because the file may be fine.
                let mut b = [0u8; RECORD_LEN];
                let record = match s.len(key) {
                    Err(_) => {
                        done.kept_unsure += 1;
                        continue;
                    }
                    Ok(len) if len != RECORD_LEN => None,
                    Ok(_) => {
                        if s.read(key, 0, &mut b) != Ok(RECORD_LEN) {
                            done.kept_unsure += 1;
                            continue;
                        }
                        Record::decode(&b).ok().filter(|r| r.generation == g)
                    }
                };
                let Some(r) = record else {
                    s.remove(key)?;
                    done.metadata += 1;
                    continue;
                };
                match self.get(r.item()) {
                    Some(live) if r.generation > live.generation => done.kept_unsure += 1,
                    Some(live) if r.generation == live.generation => {}
                    _ => {
                        s.remove(key)?;
                        done.metadata += 1;
                    }
                }
            }
            after = ids[n - 1];
        }
        let mut after = 0;
        loop {
            let mut ids = [0u32; 16];
            let n = s.payload_generations(after, &mut ids)?;
            if n == 0 {
                break;
            }
            for &g in &ids[..n] {
                if !self.iter().any(|r| r.payload_generation == g) {
                    s.remove(FileKey {
                        generation: g,
                        kind: FileKind::Payload,
                    })?;
                    done.payloads += 1;
                }
            }
            after = ids[n - 1];
        }
        Ok(done)
    }
}

/// Tokens and images are both pictures on the card (ADR 028): same record
/// flag, same 20-item shelf, same display path.
fn picture(kind: &str) -> bool {
    matches!(kind, "token" | "image")
}

/// Deletes every article and token on the card: all metadata, oldest first,
/// then all payloads. Oldest first means each item's newest record is its last
/// file to go, so a power cut part-way leaves the card in a state that loads
/// (some items gone, the rest intact) and never brings back a deleted item.
/// Unlike `collect_garbage`, a file that cannot be read still goes.
pub fn wipe(s: &mut impl Catalog) -> Result<Collected, StoreError> {
    let mut done = Collected::default();
    for kind in [FileKind::Metadata, FileKind::Payload] {
        let mut after = 0;
        loop {
            let mut ids = [0u32; 16];
            let n = match kind {
                FileKind::Metadata => s.generations(after, &mut ids)?,
                FileKind::Payload => s.payload_generations(after, &mut ids)?,
            };
            if n == 0 {
                break;
            }
            for &generation in &ids[..n] {
                s.remove(FileKey { generation, kind })?;
                match kind {
                    FileKind::Metadata => done.metadata += 1,
                    FileKind::Payload => done.payloads += 1,
                }
            }
            after = ids[n - 1];
        }
    }
    Ok(done)
}

/// A token shown on the panel whose `displayed` ack is still owed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    job: [u8; 26],
    item: [u8; 48],
    item_len: u8,
    sha: [u8; 64],
}

impl Shown {
    pub fn new(job: &str, item: &str, sha: &str) -> Result<Self, proto::Error> {
        proto::job_id(job)?;
        proto::item_id(item)?;
        proto::hash(sha)?;
        let mut s = Self {
            job: [0; 26],
            item: [0; 48],
            item_len: item.len() as u8,
            sha: [0; 64],
        };
        s.job.copy_from_slice(job.as_bytes());
        s.item[..item.len()].copy_from_slice(item.as_bytes());
        s.sha.copy_from_slice(sha.as_bytes());
        Ok(s)
    }
    pub fn job(&self) -> &str {
        core::str::from_utf8(&self.job).unwrap_or("")
    }
    pub fn item(&self) -> &str {
        core::str::from_utf8(&self.item[..self.item_len as usize]).unwrap_or("")
    }
    pub fn sha(&self) -> &str {
        core::str::from_utf8(&self.sha).unwrap_or("")
    }
}

/// What a session did. Partial progress is kept when `result` is an error.
#[derive(Debug)]
pub struct Report {
    /// Companion time from `hello`, in Unix seconds.
    pub time: Option<u64>,
    /// New payloads written to the card.
    pub received: usize,
    pub deleted: usize,
    /// Jobs answered with `rejected`.
    pub rejected: usize,
    /// The last committed `show` token, to display after the session.
    pub show: Option<Shown>,
    /// The owed `displayed` ack was sent.
    pub displayed: bool,
    /// The session used the internet route (the LAN address failed or there
    /// is none), not the home LAN.
    pub remote: bool,
    pub result: Result<(), Error>,
}

impl Default for Report {
    fn default() -> Self {
        Self {
            time: None,
            received: 0,
            deleted: 0,
            rejected: 0,
            show: None,
            displayed: false,
            remote: false,
            result: Ok(()),
        }
    }
}

fn digest(hex: &str) -> Result<[u8; 32], proto::Error> {
    proto::hash(hex)?;
    let mut out = [0u8; 32];
    let b = hex.as_bytes();
    let nibble = |c: u8| if c <= b'9' { c - b'0' } else { c - b'a' + 10 };
    for (i, o) in out.iter_mut().enumerate() {
        *o = nibble(b[i * 2]) << 4 | nibble(b[i * 2 + 1]);
    }
    Ok(out)
}

/// The status code from an HTTP/1.x status line at the start of `head`, if
/// one is there in full. Used only to describe a failure.
fn status_line(head: &[u8]) -> Option<u16> {
    let rest = head
        .strip_prefix(b"HTTP/1.1 ")
        .or_else(|| head.strip_prefix(b"HTTP/1.0 "))?;
    let digits = rest.get(..3)?;
    if rest.get(3) != Some(&b' ') || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    digits
        .iter()
        .try_fold(0u16, |n, d| Some(n * 10 + u16::from(d - b'0')))
}

struct Client<'a, N> {
    net: &'a mut N,
    companion: &'a Companion<'a>,
    /// Set once the LAN address has failed (or there is none): the rest of the
    /// session uses the remote name.
    remote: bool,
}

impl<N: Net> Client<'_, N> {
    /// One request on its own connection. A 200 body goes to `consume` in
    /// pieces; other bodies are read, capped and dropped.
    async fn exchange(
        &mut self,
        route: Route<'_>,
        body: &[u8],
        cap: usize,
        consume: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<u16, Error> {
        let signed = self.open().await?;
        let r = self.exchange_open(signed, route, body, cap, consume).await;
        // A signed connection stays open for the next request (a TLS handshake
        // is about a second); any failure closes it. LAN requests close each time.
        if !(signed && r.is_ok()) {
            self.net.close().await;
        }
        r
    }

    /// Opens a connection: the LAN address first, if there is one and it has
    /// not failed; otherwise the remote name. Returns whether it is signed.
    async fn open(&mut self) -> Result<bool, Error> {
        if !self.remote
            && let Some(a) = self.companion.address
        {
            match self
                .net
                .open(Target::Lan {
                    ip: a.ip,
                    port: a.port,
                })
                .await
            {
                Ok(()) => return Ok(false),
                Err(e) if self.companion.remote.is_none() => return Err(e.into()),
                // A timed-out connect can leave the socket still connecting
                // (the LAN address from a phone hotspot): reset it before the
                // remote name gets the same socket.
                Err(_) => self.net.close().await,
            }
        }
        let r = self.companion.remote.ok_or(NetError::Connect)?;
        self.remote = true;
        self.net
            .open(Target::Remote {
                host: r.host,
                port: r.port,
            })
            .await?;
        Ok(true)
    }

    async fn exchange_open(
        &mut self,
        signed: bool,
        route: Route<'_>,
        body: &[u8],
        cap: usize,
        consume: &mut dyn FnMut(&[u8]) -> Result<(), Error>,
    ) -> Result<u16, Error> {
        let mut head = [0u8; HEAD_CAP];
        let key = sign::key(self.companion.token);
        let mut nonce = [0u8; sign::NONCE_LEN];
        let n = if signed {
            self.net.random(&mut nonce);
            let r = self.companion.remote.ok_or(NetError::Connect)?;
            proto::http::signed_request(
                r,
                self.companion.access,
                &key,
                &nonce,
                route,
                body,
                &mut head,
            )?
        } else {
            let a = self.companion.address.ok_or(NetError::Connect)?;
            proto::http::request(a, self.companion.token, route, body.len(), &mut head)?
        };
        self.net.send(&head[..n]).await?;
        if !body.is_empty() {
            self.net.send(body).await?;
        }
        let mut used = 0;
        let h = loop {
            let got = self.net.recv(&mut head[used..]).await?;
            if got == 0 {
                return Err(proto::Error::Truncated.into());
            }
            used += got;
            match ResponseHead::parse(&head[..used], cap.max(ERROR_BODY_CAP)) {
                Ok(Some(h)) => break h,
                Ok(None) => {}
                // An edge error page can overflow the head buffer; say who
                // answered rather than just "too large".
                Err(e) => match status_line(&head[..used]) {
                    Some(code) if signed && code != 200 => return Err(Error::Edge(code)),
                    _ => return Err(e.into()),
                },
            }
        };
        if h.status == 200 && h.content_length > cap {
            return Err(proto::Error::TooLarge.into());
        }
        // A signed exchange must carry a MAC; a plain one is not expected to.
        if signed && h.mac.is_none() {
            return Err(if h.status == 200 {
                Error::Signature
            } else {
                Error::Edge(h.status)
            });
        }
        let ok = h.status == 200;
        let mut length = Body::new(h);
        let mut digest = Sha256::new();
        let early = &head[h.head_length..used];
        length.accept(early)?;
        digest.update(early);
        if ok && !early.is_empty() {
            consume(early)?;
        }
        let mut remaining = h.content_length - early.len();
        let mut buf = [0u8; 512];
        while remaining > 0 {
            let want = remaining.min(buf.len());
            let got = self.net.recv(&mut buf[..want]).await?;
            if got == 0 {
                return Err(proto::Error::Truncated.into());
            }
            length.accept(&buf[..got])?;
            digest.update(&buf[..got]);
            if ok {
                consume(&buf[..got])?;
            }
            remaining -= got;
        }
        length.finish()?;
        // LAN, `Connection: close`: anything after the body is an overrun. A
        // kept-alive signed connection has no end to wait for.
        if !signed {
            let mut extra = [0u8; 1];
            if self.net.recv(&mut extra).await? != 0 {
                return Err(proto::Error::TooLarge.into());
            }
        }
        // Nothing the reply said counts until its MAC checks. (Bytes already
        // handed to `consume` are only used after this returns `Ok`.)
        if signed {
            let nonce = nonce_hex(&nonce);
            let nonce = core::str::from_utf8(&nonce).map_err(|_| Error::Signature)?;
            let want = sign::response_mac(&key, nonce, h.status, &digest.finalize().into());
            match h.mac {
                Some(got) if sign::same(&got, &want) => {}
                _ => return Err(Error::Signature),
            }
        }
        Ok(h.status)
    }

    /// The whole 200 body into `out`; returns its length.
    async fn fetch(
        &mut self,
        step: Step,
        route: Route<'_>,
        body: &[u8],
        out: &mut [u8],
    ) -> Result<usize, Error> {
        let mut n = 0;
        let status = self
            .exchange(route, body, out.len(), &mut |b| {
                out[n..n + b.len()].copy_from_slice(b);
                n += b.len();
                Ok(())
            })
            .await?;
        if status != 200 {
            return Err(Error::Status(step, status));
        }
        Ok(n)
    }

    async fn ack(&mut self, job: &str, ack: Ack<'_>) -> Result<(), Error> {
        let mut body = [0u8; ERROR_BODY_CAP];
        let n = ack.write(&mut body)?;
        let status = self
            .exchange(Route::Ack(job), &body[..n], 0, &mut |_| Ok(()))
            .await?;
        match status {
            204 | 404 => Ok(()),
            s => Err(Error::Status(Step::Ack, s)),
        }
    }
}

/// Runs one session: `hello`, the owed `displayed` ack, then jobs until the
/// list is empty or something fails. `index` must come from `Index::load` on
/// the same store and is kept up to date. Wi-Fi, the display, the RTC and
/// token eviction are the caller's.
pub async fn run(
    net: &mut impl Net,
    store: &mut impl Catalog,
    index: &mut Index,
    companion: &Companion<'_>,
    hello: &Hello<'_>,
    owed: Option<&Shown>,
) -> Report {
    let mut report = Report::default();
    let mut client = Client {
        net,
        companion,
        remote: false,
    };
    report.result = session(&mut client, store, index, hello, owed, &mut report).await;
    report.remote = client.remote;
    client.net.close().await;
    report
}

async fn session<N: Net>(
    client: &mut Client<'_, N>,
    store: &mut impl Catalog,
    index: &mut Index,
    hello: &Hello<'_>,
    owed: Option<&Shown>,
    report: &mut Report,
) -> Result<(), Error> {
    let mut line = [0u8; 161];
    let n = hello.write(&mut line)?;
    let mut reply = [0u8; ERROR_BODY_CAP];
    let got = client
        .fetch(Step::Hello, Route::Hello, &line[..n], &mut reply)
        .await?;
    report.time = Some(proto::lines::parse_time(&reply[..got])?);
    if let Some(old) = owed {
        client
            .ack(old.job(), Ack::Committed(Some(old.sha())))
            .await?;
        client.ack(old.job(), Ack::Displayed(old.sha())).await?;
        report.displayed = true;
    }
    let mut list = [0u8; JOBS_CAP];
    loop {
        let got = client
            .fetch(Step::Jobs, Route::Jobs, &[], &mut list)
            .await?;
        let batch = Jobs::parse(&list[..got])?;
        if batch.jobs.iter().all(Option::is_none) {
            return Ok(());
        }
        for job in batch.jobs.iter().flatten() {
            if !job.supported() {
                client.ack(job.id, Ack::Rejected("unsupported")).await?;
                report.rejected += 1;
                continue;
            }
            if job.kind == "delete" {
                if let Some(old) = index.get(job.item).cloned() {
                    let generation = store.next_generation()?;
                    let gone = storage::delete(store, &old, generation)?;
                    index.apply(gone)?;
                    report.deleted += 1;
                }
                client.ack(job.id, Ack::Committed(None)).await?;
                continue;
            }
            let hash = job.sha256.ok_or(proto::Error::Invalid)?;
            let expected = digest(hash)?;
            // Already held (a lost ack): skip the download if it still verifies.
            let held = match index.get(job.item) {
                Some(r) if r.sha256 == expected => storage::verify(store, r).is_ok(),
                _ => false,
            };
            if !held {
                let no_room = index.get(job.item).is_none()
                    && if picture(job.kind) {
                        index.is_full()
                    } else {
                        !index.make_room_for_article(store)?
                    };
                if no_room {
                    client.ack(job.id, Ack::Rejected("storage")).await?;
                    report.rejected += 1;
                    continue;
                }
                let generation = store.next_generation()?;
                let record = Record::new(
                    job.item,
                    generation,
                    job.len as u32,
                    expected,
                    picture(job.kind),
                )?;
                let mut stage = Stage::begin(store, record)?;
                let mut write_failed = false;
                let status = client
                    .exchange(Route::Payload(job.id), &[], job.len, &mut |b| {
                        stage.write(store, b).map_err(|e| {
                            write_failed = true;
                            Error::Storage(e)
                        })
                    })
                    .await;
                let status = match status {
                    Err(e) if write_failed => {
                        client.ack(job.id, Ack::Rejected("storage")).await?;
                        report.rejected += 1;
                        return Err(e);
                    }
                    other => other?,
                };
                if status == 404 {
                    // Cancelled: the staged generation is abandoned, no ack.
                    continue;
                }
                if status != 200 {
                    return Err(Error::Status(Step::Payload, status));
                }
                match stage.commit(store) {
                    Ok(r) => index.apply(r)?,
                    Err(StoreError::Corrupt) => {
                        client.ack(job.id, Ack::Rejected("checksum")).await?;
                        report.rejected += 1;
                        return Err(Error::Checksum);
                    }
                    Err(e) => {
                        client.ack(job.id, Ack::Rejected("storage")).await?;
                        report.rejected += 1;
                        return Err(Error::Storage(e));
                    }
                }
                report.received += 1;
            }
            if job.show {
                report.show = Some(Shown::new(job.id, job.item, hash)?);
            }
            client.ack(job.id, Ack::Committed(Some(hash))).await?;
        }
    }
}

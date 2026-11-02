use crate::{Error, Result, hash, invalid, now};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::{
    path::Path,
    sync::{Mutex, MutexGuard},
    time::Duration,
};
pub type Pending = (Job, String, Option<Vec<u8>>);
/// Five attempts with 30, 60, 120 and 240 s gaps cover about 7.5 minutes.
pub const PREPARE_ATTEMPTS: u32 = 5;
const RETRY_BASE: i64 = 30;
pub struct Database {
    connection: Mutex<Connection>,
    pub quota: u64,
}
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub job: String,
    pub item: String,
    pub kind: String,
    pub state: String,
    pub sha256: Option<String>,
    pub len: usize,
    pub show: bool,
    pub delivery_started: bool,
    pub created: i64,
    /// Article title, card name or link; for display only.
    pub title: Option<String>,
}
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        job: r.get(0)?,
        item: r.get(1)?,
        kind: r.get(2)?,
        state: r.get(3)?,
        sha256: r.get(4)?,
        len: r.get::<_, u32>(5)? as usize,
        show: r.get(6)?,
        delivery_started: r.get(7)?,
        created: r.get(8)?,
        title: r.get(9)?,
    })
}
const COLS: &str =
    "id,item,kind,state,sha256,COALESCE(length(payload),0),show,delivery_started,created,title";
pub struct Submission<'a> {
    pub id: &'a str,
    pub canonical: &'a str,
    pub kind: &'a str,
    pub item: Option<&'a str>,
    pub show: bool,
    pub prepared: Option<&'a [u8]>,
    pub title: Option<&'a str>,
}
/// The item ID for a new job. A token with a name carries it, so the device
/// can list it: `t-goblin-warrior-0123456789ab` (`x3_core::library::token_name`).
/// Other jobs stay `a-<uuid>` / `t-<uuid>`. Always a valid `x3_proto::item_id`.
pub fn new_item(kind: &str, title: Option<&str>, id: uuid::Uuid) -> String {
    let hex = id.simple().to_string();
    let prefix = match kind {
        "token" => "t-",
        "image" => "i-",
        _ => return format!("a-{hex}"),
    };
    let slug = slug(title.unwrap_or(""), 48 - prefix.len() - "-".len() - 12);
    if slug.is_empty() {
        return format!("{prefix}{hex}");
    }
    format!("{prefix}{slug}-{}", &hex[..12])
}

/// Lower-case ASCII letters and digits joined by single hyphens, at most `max`
/// bytes, cut at a word end where possible. Accented Latin letters lose
/// their accents; other characters separate words.
fn slug(name: &str, max: usize) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in name.chars().flat_map(char::to_lowercase) {
        if let Some(pair) = match c {
            'æ' => Some("ae"),
            'œ' => Some("oe"),
            'ß' => Some("ss"),
            _ => None,
        } {
            if gap {
                out.push('-');
                gap = false;
            }
            out.push_str(pair);
            continue;
        }
        let c = match c {
            'a'..='z' | '0'..='9' => c,
            'à'..='å' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'è'..='ë' | 'ę' | 'ě' => 'e',
            'ì'..='ï' => 'i',
            'ł' => 'l',
            'ñ' | 'ń' | 'ň' => 'n',
            'ò'..='ö' | 'ø' => 'o',
            'ś' | 'š' => 's',
            'ù'..='ü' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            '\'' | '’' => continue,
            _ => {
                gap = !out.is_empty();
                continue;
            }
        };
        if gap {
            out.push('-');
            gap = false;
        }
        out.push(c);
    }
    if out.len() > max {
        let cut = out[..=max].rfind('-').filter(|&n| n > 0).unwrap_or(max);
        out.truncate(cut);
    }
    out
}

impl Database {
    pub fn open(path: impl AsRef<Path>, quota: u64) -> Result<Self> {
        let c = Connection::open(path)?;
        c.busy_timeout(Duration::from_secs(5))?;
        let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !matches!(version, 0..=5) {
            return Err(Error(503, "unsupported_database"));
        }

        c.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON; PRAGMA journal_size_limit=4194304;
  CREATE TABLE IF NOT EXISTS devices(id INTEGER PRIMARY KEY CHECK(id=1),name TEXT NOT NULL,token_hash TEXT NOT NULL,last_contact INTEGER,hello TEXT);
  CREATE TABLE IF NOT EXISTS jobs(id TEXT PRIMARY KEY,submission_id TEXT UNIQUE,canonical TEXT NOT NULL,item TEXT NOT NULL,kind TEXT NOT NULL,state TEXT NOT NULL,payload BLOB,sha256 TEXT,show INTEGER NOT NULL DEFAULT 0,delivery_started INTEGER NOT NULL DEFAULT 0,lease_until INTEGER,created INTEGER NOT NULL,updated INTEGER NOT NULL);
  CREATE TABLE IF NOT EXISTS history(job TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,state TEXT NOT NULL,time INTEGER NOT NULL);
  CREATE INDEX IF NOT EXISTS jobs_state ON jobs(state,created);
  CREATE INDEX IF NOT EXISTS jobs_item ON jobs(item);")?;
        // Version 2 adds bounded retries for transient preparation failures.
        if version < 2 {
            c.execute_batch(
                "BEGIN; ALTER TABLE jobs ADD COLUMN attempts INTEGER NOT NULL DEFAULT 0;
  ALTER TABLE jobs ADD COLUMN retry_at INTEGER; PRAGMA user_version=2; COMMIT;",
            )?;
        }
        // Version 3 adds a display title (ADR 015).
        if version < 3 {
            c.execute_batch(
                "BEGIN; ALTER TABLE jobs ADD COLUMN title TEXT; PRAGMA user_version=3; COMMIT;",
            )?;
        }
        // Version 4 keeps finished tokens and pictures to send again.
        if version < 4 {
            c.execute_batch(
                "BEGIN; CREATE TABLE IF NOT EXISTS saved(sha256 TEXT PRIMARY KEY,kind TEXT NOT NULL,title TEXT,payload BLOB NOT NULL,created INTEGER NOT NULL,used INTEGER NOT NULL);
  CREATE INDEX IF NOT EXISTS saved_used ON saved(used); PRAGMA user_version=4; COMMIT;",
            )?;
        }
        // Version 5 fills the shelf once with what the reader already has,
        // newest first, so it does not start empty (ADR 029).
        if version < 5 {
            let tx = c.unchecked_transaction()?;
            tx.execute(
                "INSERT OR IGNORE INTO saved(sha256,kind,title,payload,created,used)
  SELECT sha256,kind,title,payload,updated,updated FROM jobs
  WHERE kind IN ('token','image') AND state IN ('delivered','displayed')
  AND payload IS NOT NULL AND sha256 IS NOT NULL ORDER BY updated DESC,rowid DESC",
                [],
            )?;
            trim(&tx)?;
            tx.execute_batch("PRAGMA user_version=5")?;
            tx.commit()?;
        }
        if quota < 65536 {
            return Err(invalid());
        }
        c.pragma_update(
            None,
            "max_page_count",
            i64::try_from(quota / 4096).map_err(|_| invalid())?,
        )?;
        Ok(Self {
            connection: Mutex::new(c),
            quota,
        })
    }
    pub fn restart(&self) -> Result<()> {
        self.lock()?.execute(
            "UPDATE jobs SET state='accepted' WHERE state='preparing'",
            [],
        )?;
        Ok(())
    }
    pub fn lock(&self) -> Result<MutexGuard<'_, Connection>> {
        self.connection
            .lock()
            .map_err(|_| Error(503, "unavailable"))
    }
    pub fn add_device(&self, name: &str, token: &str) -> Result<()> {
        if name.is_empty() || name.len() > 64 || name.chars().any(char::is_control) {
            return Err(invalid());
        }
        x3_proto::config::token(token).map_err(|_| invalid())?;
        let c = self.lock()?;
        if c.query_row("SELECT count(*) FROM devices", [], |r| r.get::<_, i64>(0))? > 0 {
            return Err(Error(409, "conflict"));
        }
        c.execute(
            "INSERT INTO devices(id,name,token_hash) VALUES(1,?1,?2)",
            params![name, hash(token.as_bytes())],
        )?;
        Ok(())
    }
    pub fn device_auth(&self, token: &str) -> Result<bool> {
        if x3_proto::config::token(token).is_err() {
            return Ok(false);
        }
        let expected: Option<String> = self
            .lock()?
            .query_row("SELECT token_hash FROM devices WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(expected.is_some_and(|v| v == hash(token.as_bytes())))
    }
    /// The signing key for the device (ADR 026): the stored token hash, as bytes.
    pub fn device_key(&self) -> Result<Option<x3_proto::sign::Key>> {
        let stored: Option<String> = self
            .lock()?
            .query_row("SELECT token_hash FROM devices WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        Ok(stored.and_then(|h| x3_proto::sign::key_from_hex(&h)))
    }
    pub fn hello(&self, body: &str) -> Result<()> {
        self.lock()?.execute(
            "UPDATE devices SET last_contact=?1,hello=?2 WHERE id=1",
            params![now(), body],
        )?;
        Ok(())
    }
    pub fn device(&self) -> Result<serde_json::Value> {
        Ok(self.lock()?.query_row("SELECT name,last_contact,hello FROM devices WHERE id=1",[],|r|Ok(serde_json::json!({"name":r.get::<_,String>(0)?,"last_contact":r.get::<_,Option<i64>>(1)?,"hello":r.get::<_,Option<String>>(2)?}))).optional()?.unwrap_or(serde_json::Value::Null))
    }
    pub fn repeat(&self, id: &str, canonical: &str) -> Result<Option<Job>> {
        let c = self.lock()?;
        let found: Option<(String, String)> = c
            .query_row(
                "SELECT id,canonical FROM jobs WHERE submission_id=?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        match found {
            None => Ok(None),
            Some((job, old)) => {
                if old != canonical {
                    return Err(Error(409, "conflict"));
                }
                Ok(Some(c.query_row(
                    &format!("SELECT {COLS} FROM jobs WHERE id=?1"),
                    [job],
                    row,
                )?))
            }
        }
    }
    pub fn submit(&self, s: Submission<'_>) -> Result<(Job, bool)> {
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let old: Option<(String, String)> = tx
            .query_row(
                "SELECT id,canonical FROM jobs WHERE submission_id=?1",
                [s.id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        if let Some((id, canonical)) = old {
            if canonical != s.canonical {
                return Err(Error(409, "conflict"));
            }
            let j = tx.query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [id], row)?;
            return Ok((j, false));
        }
        let device: bool =
            tx.query_row("SELECT EXISTS(SELECT 1 FROM devices)", [], |r| r.get(0))?;
        if !device {
            return Err(Error(503, "unavailable"));
        }
        // Only jobs still waiting for preparation reserve their maximum payload.
        let used: i64 = tx.query_row(
            "SELECT COALESCE(sum(length(canonical)+COALESCE(length(payload),CASE WHEN state NOT IN ('accepted','preparing') THEN 0 WHEN kind='token' THEN 104560 WHEN kind='article' THEN 264476 ELSE 0 END)+1024),0) FROM jobs",
            [],
            |r| r.get(0),
        )?;
        // Reserve the maximum eventual payload before accepting a pending job.
        let reserve = if matches!(s.kind, "token" | "image") {
            x3_proto::TOKEN_CAP
        } else {
            x3_proto::ARTICLE_CAP
        };
        if used as u64 + s.canonical.len() as u64 + reserve as u64 + 1024 > self.quota {
            return Err(Error(507, "storage_full"));
        }
        let item = if let Some(item) = s.item {
            x3_proto::item_id(item).map_err(|_| invalid())?;
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM jobs WHERE item=?1 AND kind='article')",
                [item],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error(404, "not_found"));
            }
            item.to_owned()
        } else {
            new_item(s.kind, s.title, uuid::Uuid::new_v4())
        };
        let id = ulid::Ulid::generate().to_string();
        let time = now();
        let sha = s.prepared.map(hash);
        tx.execute("INSERT INTO jobs(id,submission_id,canonical,item,kind,state,payload,sha256,show,created,updated,title) VALUES(?1,?2,?3,?4,?5,'accepted',?6,?7,?8,?9,?9,?10)",params![id,s.id,s.canonical,item,s.kind,s.prepared,sha,s.show,time,s.title])?;
        tx.execute(
            "INSERT INTO history VALUES(?1,'accepted',?2)",
            params![id, time],
        )?;
        let j = tx.query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [&id], row)?;
        tx.commit()?;
        Ok((j, true))
    }
    pub fn job(&self, id: &str) -> Result<Job> {
        self.lock()?
            .query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [id], row)
            .optional()?
            .ok_or(Error(404, "not_found"))
    }
    pub fn history(&self, id: &str) -> Result<Vec<serde_json::Value>> {
        self.job(id)?;
        let c = self.lock()?;
        let mut q = c.prepare("SELECT state,time FROM history WHERE job=?1 ORDER BY rowid")?;
        Ok(q.query_map([id], |r| {
            Ok(serde_json::json!({"state":r.get::<_,String>(0)?,"time":r.get::<_,i64>(1)?}))
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// Newest first. `before` is a job ID: only older jobs are listed. The
    /// flag says whether more are left after this page.
    pub fn list(
        &self,
        state: Option<&str>,
        limit: usize,
        before: Option<&str>,
    ) -> Result<(Vec<Job>, bool)> {
        let c = self.lock()?;
        let mut q = c.prepare(&format!(
            "SELECT {COLS} FROM jobs WHERE (?1 IS NULL OR state=?1)
  AND (?3 IS NULL OR rowid<(SELECT rowid FROM jobs WHERE id=?3)) ORDER BY rowid DESC LIMIT ?2"
        ))?;
        let mut jobs: Vec<Job> = q
            .query_map(params![state, limit as i64 + 1, before], row)?
            .collect::<rusqlite::Result<_>>()?;
        let more = jobs.len() > limit;
        jobs.truncate(limit);
        Ok((jobs, more))
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let j = tx
            .query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [id], row)
            .optional()?
            .ok_or(Error(404, "not_found"))?;
        if j.delivery_started || matches!(j.state.as_str(), "delivered" | "displayed") {
            return Err(Error(409, "conflict"));
        }
        if j.state != "cancelled" {
            transition(&tx, id, "cancelled")?;
        }
        tx.commit()?;
        Ok(())
    }
    pub fn queue_delete(&self, item: &str) -> Result<Job> {
        x3_proto::item_id(item).map_err(|_| invalid())?;
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM jobs WHERE item=?1)",
            [item],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(Error(404, "not_found"));
        }
        let id = ulid::Ulid::generate().to_string();
        let time = now();
        tx.execute("INSERT INTO jobs(id,canonical,item,kind,state,created,updated,title) VALUES(?1,'',?2,'delete','ready',?3,?3,(SELECT title FROM jobs WHERE item=?2 AND kind<>'delete' AND title IS NOT NULL ORDER BY rowid DESC LIMIT 1))",params![id,item,time])?;
        tx.execute(
            "INSERT INTO history VALUES(?1,'ready',?2)",
            params![id, time],
        )?;
        let j = tx.query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [id], row)?;
        tx.commit()?;
        Ok(j)
    }
    pub fn pending(&self) -> Result<Option<Pending>> {
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let id: Option<String> = tx
            .query_row(
                "SELECT id FROM jobs WHERE state='accepted' AND (retry_at IS NULL OR retry_at<=?1) ORDER BY rowid LIMIT 1",
                [now()],
                |r| r.get(0),
            )
            .optional()?;
        let Some(id) = id else { return Ok(None) };
        transition(&tx, &id, "preparing")?;
        let j = tx.query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [&id], row)?;
        let (canonical, payload) = tx.query_row(
            "SELECT canonical,payload FROM jobs WHERE id=?1",
            [&id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        tx.commit()?;
        Ok(Some((j, canonical, payload)))
    }
    /// Upstream (502) and capacity (503) failures are retried with backoff;
    /// anything else, or the last attempt, fails the job.
    pub fn prepared(&self, id: &str, outcome: std::result::Result<&[u8], Error>) -> Result<()> {
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let (state, attempts): (String, u32) =
            tx.query_row("SELECT state,attempts FROM jobs WHERE id=?1", [id], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if state != "preparing" {
            return Ok(());
        }
        match outcome {
            Ok(b) => {
                tx.execute(
                    "UPDATE jobs SET payload=?2,sha256=?3 WHERE id=?1",
                    params![id, b, hash(b)],
                )?;
                transition(&tx, id, "ready")?;
            }
            Err(Error(502 | 503, _)) if attempts + 1 < PREPARE_ATTEMPTS => {
                tx.execute(
                    "UPDATE jobs SET attempts=?2,retry_at=?3 WHERE id=?1",
                    params![id, attempts + 1, now() + (RETRY_BASE << attempts)],
                )?;
                transition(&tx, id, "accepted")?;
            }
            Err(_) => transition(&tx, id, "failed")?,
        }
        tx.commit()?;
        Ok(())
    }
    /// Replaces the display title, for example after a link is read.
    pub fn set_title(&self, id: &str, title: &str) -> Result<()> {
        self.lock()?
            .execute("UPDATE jobs SET title=?2 WHERE id=?1", params![id, title])?;
        Ok(())
    }
    pub fn device_jobs(&self) -> Result<(Vec<Job>, bool)> {
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let expired: Vec<String> = {
            let mut q =
                tx.prepare("SELECT id FROM jobs WHERE state='sending' AND lease_until<=?1")?;
            q.query_map([now()], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?
        };
        for id in expired {
            transition(&tx, &id, "ready")?;
        }
        // Do not overtake an earlier pending change to the same item.
        let mut jobs: Vec<Job> = {
            // Image jobs only go to a device whose last hello lists `im1`
            // (ADR 028); older firmware is not offered them.
            let hello: Option<String> = tx
                .query_row("SELECT hello FROM devices WHERE id=1", [], |r| r.get(0))
                .optional()?
                .flatten();
            let images = hello
                .as_deref()
                .and_then(|h| x3_proto::lines::Hello::parse(h.as_bytes()).ok())
                .is_some_and(|h| h.takes("im1"));
            let mut q=tx.prepare(&format!("SELECT {COLS} FROM jobs j WHERE state='ready' AND (j.kind<>'image' OR ?1) AND NOT EXISTS(SELECT 1 FROM jobs old WHERE old.item=j.item AND old.rowid<j.rowid AND old.state IN ('accepted','preparing','ready','sending')) ORDER BY rowid LIMIT 17"))?;
            q.query_map([images], row)?
                .collect::<rusqlite::Result<_>>()?
        };
        let more = jobs.len() > 16;
        jobs.truncate(16);
        for j in &mut jobs {
            if j.kind == "delete" {
                tx.execute(
                    "UPDATE jobs SET delivery_started=1,lease_until=?2 WHERE id=?1",
                    params![j.job, now() + 300],
                )?;
                transition(&tx, &j.job, "sending")?;
                j.delivery_started = true;
                j.state = "sending".into();
            }
        }
        tx.commit()?;
        Ok((jobs, more))
    }
    pub fn payload(&self, id: &str, deliver: bool) -> Result<Vec<u8>> {
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let j = tx
            .query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [id], row)
            .optional()?
            .ok_or(Error(404, "not_found"))?;
        if j.kind == "delete"
            || matches!(j.state.as_str(), "cancelled" | "failed")
            || (deliver && !matches!(j.state.as_str(), "ready" | "sending"))
        {
            return Err(Error(404, "not_found"));
        }
        let b: Option<Vec<u8>> =
            tx.query_row("SELECT payload FROM jobs WHERE id=?1", [id], |r| r.get(0))?;
        let b = b.ok_or(if matches!(j.state.as_str(), "accepted" | "preparing") {
            Error(503, "busy")
        } else {
            Error(404, "not_found")
        })?;
        if deliver {
            tx.execute(
                "UPDATE jobs SET delivery_started=1,lease_until=?2 WHERE id=?1",
                params![id, now() + 300],
            )?;
            if j.state != "sending" {
                transition(&tx, id, "sending")?;
            }
        }
        tx.commit()?;
        Ok(b)
    }
    pub fn ack(&self, id: &str, ack: x3_proto::lines::Ack<'_>) -> Result<()> {
        use x3_proto::lines::Ack;
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let j = tx
            .query_row(&format!("SELECT {COLS} FROM jobs WHERE id=?1"), [id], row)
            .optional()?
            .ok_or(Error(404, "not_found"))?;
        if j.state == "cancelled" {
            return Err(Error(404, "not_found"));
        }
        let state = match ack {
            Ack::Committed(h) => {
                if h != j.sha256.as_deref() {
                    return Err(Error(409, "conflict"));
                }
                if !matches!(
                    j.state.as_str(),
                    "ready" | "sending" | "delivered" | "displayed"
                ) {
                    return Err(Error(409, "conflict"));
                }
                if j.state == "displayed" {
                    "displayed"
                } else {
                    "delivered"
                }
            }
            Ack::Displayed(h) => {
                if !j.show
                    || !matches!(j.kind.as_str(), "token" | "image")
                    || Some(h) != j.sha256.as_deref()
                    || !matches!(j.state.as_str(), "delivered" | "displayed")
                {
                    return Err(Error(409, "conflict"));
                }
                "displayed"
            }
            Ack::Rejected(_) => {
                if matches!(j.state.as_str(), "delivered" | "displayed") {
                    return Err(Error(409, "conflict"));
                }
                "failed"
            }
        };
        if j.state != state {
            transition(&tx, id, state)?;
        }
        tx.execute("UPDATE jobs SET delivery_started=1 WHERE id=?1", [id])?;
        if matches!(state, "delivered" | "displayed")
            && matches!(j.kind.as_str(), "token" | "image")
        {
            keep(&tx, id)?;
        }
        tx.commit()?;
        Ok(())
    }
    /// Saved tokens and pictures, most recently used first.
    pub fn saved(&self) -> Result<Vec<Saved>> {
        let c = self.lock()?;
        let mut q = c.prepare(
            "SELECT sha256,kind,title,length(payload),created,used FROM saved ORDER BY used DESC,rowid DESC",
        )?;
        Ok(q.query_map([], |r| {
            Ok(Saved {
                id: r.get(0)?,
                kind: r.get(1)?,
                title: r.get(2)?,
                len: r.get::<_, u32>(3)? as usize,
                created: r.get(4)?,
                used: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?)
    }
    /// One saved entry and its payload.
    pub fn saved_payload(&self, id: &str) -> Result<(Saved, Vec<u8>)> {
        self.lock()?
            .query_row(
                "SELECT sha256,kind,title,length(payload),created,used,payload FROM saved WHERE sha256=?1",
                [id],
                |r| {
                    Ok((
                        Saved {
                            id: r.get(0)?,
                            kind: r.get(1)?,
                            title: r.get(2)?,
                            len: r.get::<_, u32>(3)? as usize,
                            created: r.get(4)?,
                            used: r.get(5)?,
                        },
                        r.get(6)?,
                    ))
                },
            )
            .optional()?
            .ok_or(Error(404, "not_found"))
    }
    /// Marks a saved entry as just used, so it stays at the front.
    pub fn saved_used(&self, id: &str) -> Result<()> {
        self.lock()?.execute(
            "UPDATE saved SET used=max(used+1,?2) WHERE sha256=?1",
            params![id, now()],
        )?;
        Ok(())
    }
    pub fn forget(&self, id: &str) -> Result<()> {
        if self
            .lock()?
            .execute("DELETE FROM saved WHERE sha256=?1", [id])?
            == 0
        {
            return Err(Error(404, "not_found"));
        }
        Ok(())
    }
    pub fn backup(&self, path: &Path) -> Result<()> {
        if path.exists() {
            return Err(Error(409, "conflict"));
        }
        let p = path.to_str().ok_or_else(invalid)?;
        self.lock()?.execute("VACUUM INTO ?1", [p])?;
        Ok(())
    }
    pub fn restore_check(path: &Path) -> Result<usize> {
        let c = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let check: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if check != "ok" {
            return Err(Error(422, "invalid_backup"));
        }
        let mut q = c.prepare("SELECT payload,sha256,kind FROM jobs WHERE payload IS NOT NULL")?;
        let rows = q.query_map([], |r| {
            Ok((
                r.get::<_, Vec<u8>>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?;
        let mut count = 0;
        for row in rows {
            let (b, h, kind) = row?;
            if hash(&b) != h {
                return Err(Error(422, "invalid_backup"));
            }
            if matches!(kind.as_str(), "token" | "image") {
                x3_proto::payload::TokenHeader::payload(&b)
                    .map_err(|_| Error(422, "invalid_backup"))?;
            } else {
                x3_proto::payload::Article::decode(&b).map_err(|_| Error(422, "invalid_backup"))?;
            }
            count += 1;
        }
        Ok(count)
    }
    /// Old finished jobs are removed, except the newest delivered content job
    /// of each item still on the device. That row stays, without its payload
    /// and request text, so the item can still be replaced or removed.
    pub fn prune(&self, days: i64) -> Result<usize> {
        let cutoff = now() - days * 86400;
        let mut c = self.lock()?;
        let tx = c.transaction()?;
        let removed = tx.execute(
            "DELETE FROM jobs WHERE updated<?1 AND (state IN ('failed','cancelled')
  OR (state IN ('delivered','displayed') AND (kind='delete' OR EXISTS(SELECT 1 FROM jobs later
  WHERE later.item=jobs.item AND later.rowid>jobs.rowid AND later.state IN ('delivered','displayed')))))",
            [cutoff],
        )?;
        tx.execute(
            "UPDATE jobs SET payload=NULL,canonical='' WHERE updated<?1
  AND state IN ('delivered','displayed') AND (payload IS NOT NULL OR canonical<>'')",
            [cutoff],
        )?;
        tx.commit()?;
        Ok(removed)
    }
}
/// Most tokens and pictures kept to send again; each is at most `TOKEN_CAP`.
pub const SAVED_MAX: i64 = 50;
/// A finished token or picture, kept by its payload hash.
#[derive(Debug, Clone, Serialize)]
pub struct Saved {
    /// The payload's SHA-256: one entry per distinct image.
    pub id: String,
    pub kind: String,
    pub title: Option<String>,
    pub len: usize,
    pub created: i64,
    pub used: i64,
}
/// Copies a delivered token or picture to the saved shelf (newest `SAVED_MAX`).
fn keep(tx: &rusqlite::Transaction<'_>, id: &str) -> Result<()> {
    tx.execute(
        "INSERT INTO saved(sha256,kind,title,payload,created,used)
  SELECT sha256,kind,title,payload,?2,?2 FROM jobs WHERE id=?1 AND payload IS NOT NULL AND sha256 IS NOT NULL
  ON CONFLICT(sha256) DO UPDATE SET used=max(saved.used,excluded.used),title=COALESCE(excluded.title,saved.title)",
        params![id, now()],
    )?;
    trim(tx)
}
/// Keeps the newest `SAVED_MAX` saved entries.
fn trim(tx: &rusqlite::Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM saved WHERE sha256 NOT IN (SELECT sha256 FROM saved ORDER BY used DESC,rowid DESC LIMIT ?1)",
        [SAVED_MAX],
    )?;
    Ok(())
}
fn transition(tx: &rusqlite::Transaction<'_>, id: &str, state: &str) -> Result<()> {
    tx.execute(
        "UPDATE jobs SET state=?2,updated=?3 WHERE id=?1",
        params![id, state, now()],
    )?;
    tx.execute(
        "INSERT INTO history VALUES(?1,?2,?3)",
        params![id, state, now()],
    )?;
    Ok(())
}

#[cfg(test)]
mod item_tests {
    use super::new_item;

    fn id() -> uuid::Uuid {
        uuid::Uuid::parse_str("0123456789abcdef0123456789abcdef").unwrap()
    }

    #[test]
    fn token_ids_carry_the_name() {
        let cases = [
            (Some("Goblin Warrior"), "t-goblin-warrior-0123456789ab"),
            (Some("Treasure"), "t-treasure-0123456789ab"),
            (Some("Æther  // Spirit"), "t-aether-spirit-0123456789ab"),
            (
                Some("Smok Wawelski Żółć"),
                "t-smok-wawelski-zolc-0123456789ab",
            ),
            (Some("Elspeth's Soldier"), "t-elspeths-soldier-0123456789ab"),
            (Some("1/1 Elf"), "t-1-1-elf-0123456789ab"),
            (Some("!!!"), "t-0123456789abcdef0123456789abcdef"),
            (Some("一二三"), "t-0123456789abcdef0123456789abcdef"),
            (None, "t-0123456789abcdef0123456789abcdef"),
        ];
        for (name, want) in cases {
            let got = new_item("token", name, id());
            assert_eq!(got, want);
            assert!(x3_proto::item_id(&got).is_ok());
        }
        assert_eq!(
            new_item("article", Some("Title"), id()),
            "a-0123456789abcdef0123456789abcdef"
        );
    }

    #[test]
    fn long_names_fit_and_cut_at_a_word() {
        let long = "Eldrazi Scion of the Very Long Name Indeed That Goes On";
        let got = new_item("token", Some(long), id());
        assert!(got.len() <= 48, "{got}");
        assert!(x3_proto::item_id(&got).is_ok());
        assert!(
            got.starts_with("t-eldrazi-scion-of-the-very-long-"),
            "{got}"
        );
        assert!(!got.contains("--"));
        let one = "A".repeat(200);
        let got = new_item("token", Some(&one), id());
        assert_eq!(got.len(), 48);
        assert!(x3_proto::item_id(&got).is_ok());
    }
}

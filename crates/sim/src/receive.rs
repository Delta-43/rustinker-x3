//! Host Receive session: `x3_core::receive` over a std TCP socket, with the
//! device content on a disk image and the screen as PNGs.
use crate::disk::{Clock, Disk};
use embedded_sdmmc::{VolumeIdx, VolumeManager};
use std::{
    future::Future,
    io::{Read, Write},
    net::{Shutdown, SocketAddr, TcpStream},
    path::Path,
    pin::pin,
    task::{Context, Poll, Waker},
    time::{Duration, Instant},
};
use x3_core::{
    fat::FatStore,
    receive::{Index, Net, NetError, Shown, Target},
    storage::Store,
};
use x3_proto::{config::Companion, lines::Hello};
type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Debug) -> String {
    format!("{e:?}")
}
/// Blocking sockets behind the async `Net`: every call finishes before it
/// returns, so one poll runs the whole session.
fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => unreachable!("blocking transport never waits"),
    }
}
struct Tcp {
    socket: Option<TcpStream>,
    deadline: Instant,
    /// Test seam: where a remote name goes, over plain TCP (the simulator has
    /// no TLS). `None` refuses remote targets.
    remote: Option<SocketAddr>,
    /// The open socket is a kept-alive remote connection.
    kept: bool,
}
impl Tcp {
    fn limit(&self, max: u64) -> std::result::Result<Duration, NetError> {
        let left = self
            .deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(NetError::Timeout)?;
        Ok(left.min(Duration::from_secs(max)))
    }
    fn socket(&mut self) -> std::result::Result<&mut TcpStream, NetError> {
        self.socket.as_mut().ok_or(NetError::Io)
    }
}
fn io(e: std::io::Error) -> NetError {
    match e.kind() {
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut => NetError::Timeout,
        _ => NetError::Io,
    }
}
impl Net for Tcp {
    fn random(&mut self, out: &mut [u8]) {
        // Only the signed internet route uses nonces. Unique, not secret: the
        // simulator is a test seam.
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        let mut n = COUNTER.fetch_add(1, Ordering::Relaxed);
        for b in out.iter_mut() {
            *b = (n & 0xff) as u8;
            n = n.rotate_right(8) ^ 0x9e37_79b9_7f4a_7c15;
        }
    }
    async fn open(&mut self, to: Target<'_>) -> std::result::Result<(), NetError> {
        if let Target::Remote { .. } = to {
            if self.kept && self.socket.is_some() {
                return Ok(());
            }
            let addr = self.remote.ok_or(NetError::Connect)?;
            let t = self.limit(5)?;
            self.socket =
                Some(TcpStream::connect_timeout(&addr, t).map_err(|_| NetError::Connect)?);
            self.kept = true;
            return Ok(());
        }
        let Target::Lan { ip, port } = to else {
            return Err(NetError::Connect);
        };
        self.kept = false;
        let t = self.limit(5)?;
        let socket = TcpStream::connect_timeout(&SocketAddr::from((ip, port)), t)
            .map_err(|_| NetError::Connect)?;
        self.socket = Some(socket);
        Ok(())
    }
    async fn send(&mut self, bytes: &[u8]) -> std::result::Result<(), NetError> {
        let t = self.limit(10)?;
        let s = self.socket()?;
        s.set_write_timeout(Some(t)).map_err(io)?;
        s.write_all(bytes).map_err(io)
    }
    async fn recv(&mut self, out: &mut [u8]) -> std::result::Result<usize, NetError> {
        let t = self.limit(10)?;
        let s = self.socket()?;
        s.set_read_timeout(Some(t)).map_err(io)?;
        s.read(out).map_err(io)
    }
    async fn close(&mut self) {
        if let Some(s) = self.socket.take() {
            let _ = s.shutdown(Shutdown::Both);
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Screen {
    job: String,
    item: String,
    sha: String,
    pending: bool,
}
fn save_screen(output: &Path, screen: &Screen) -> Result<()> {
    let bytes = serde_json::to_vec(screen).map_err(err)?;
    let temp = output.join("screen.tmp");
    let mut f = std::fs::File::create(&temp).map_err(err)?;
    f.write_all(&bytes).map_err(err)?;
    f.sync_all().map_err(err)?;
    std::fs::rename(temp, output.join("screen.json")).map_err(err)
}
/// Item PNGs whose item is no longer on the card.
fn remove_stale(output: &Path, index: &Index) -> Result<()> {
    for f in std::fs::read_dir(output).map_err(err)? {
        let path = f.map_err(err)?.path();
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if path.extension().is_some_and(|e| e == "png")
            && stem != "display"
            && index.get(stem).is_none()
        {
            std::fs::remove_file(path).map_err(err)?;
        }
    }
    Ok(())
}
/// Output holds host display state and PNGs; the image holds all device content
/// and metadata. No real hardware or server configuration is accessed.
pub fn receive(config: &[u8], image: &Path, output: &Path) -> Result<usize> {
    receive_via(config, image, output, None)
}
/// `receive`, with a remote (`https://`) target sent to `remote` over plain TCP.
/// For tests of the signed route against a real server; there is no TLS here.
pub fn receive_via(
    config: &[u8],
    image: &Path,
    output: &Path,
    remote: Option<SocketAddr>,
) -> Result<usize> {
    let cfg = Companion::parse(config).map_err(err)?;
    let mut net = Tcp {
        socket: None,
        deadline: Instant::now() + Duration::from_secs(180),
        remote,
        kept: false,
    };
    std::fs::create_dir_all(output).map_err(err)?;
    let mut screen: Option<Screen> = if output.join("screen.json").exists() {
        let mut bytes = Vec::new();
        std::fs::File::open(output.join("screen.json"))
            .map_err(err)?
            .take(1025)
            .read_to_end(&mut bytes)
            .map_err(err)?;
        if bytes.len() > 1024 {
            return Err("Screen state too large".into());
        }
        let state: Screen = serde_json::from_slice(&bytes).map_err(|_| "Invalid screen state")?;
        Shown::new(&state.job, &state.item, &state.sha).map_err(err)?;
        Some(state)
    } else {
        None
    };
    let disk = Disk::open(image).map_err(err)?;
    let manager = VolumeManager::new(disk, Clock);
    let volume = manager.open_raw_volume(VolumeIdx(0)).map_err(err)?;
    let directory = manager.open_root_dir(volume).map_err(err)?;
    let mut s = FatStore {
        manager: &manager,
        directory,
    };
    let hello = Hello {
        firmware: "0.1.0-sim",
        width: 792,
        height: 528,
        formats: "tk1,ar1,im1",
        free_kib: 0,
        battery: None,
    };
    let owed = match &screen {
        Some(old) if old.pending => Some(Shown::new(&old.job, &old.item, &old.sha).map_err(err)?),
        _ => None,
    };
    let mut index = Index::load(&mut s).map_err(err)?;
    let report = block_on(x3_core::receive::run(
        &mut net,
        &mut s,
        &mut index,
        &cfg,
        &hello,
        owed.as_ref(),
    ));
    if report.time.is_none() {
        // Hello failed: nothing changed on the card or the screen.
        return report
            .result
            .and(Err(x3_core::receive::Error::Response(
                x3_proto::Error::Invalid,
            )))
            .map_err(|e| format!("{} ({e:?})", e.message()));
    }
    if report.displayed
        && let Some(old) = screen.as_mut()
    {
        old.pending = false;
        save_screen(output, old)?;
    }
    remove_stale(output, &index)?;
    for r in index.iter() {
        let mut bytes = vec![0; r.len as usize];
        if s.read(r.payload_key(), 0, &mut bytes).map_err(err)? != bytes.len() {
            return Err("Short payload read".into());
        }
        let png = if r.token {
            x3_tools::token_preview(&bytes)?
        } else {
            let a = x3_proto::payload::Article::decode(&bytes).map_err(err)?;
            crate::article(a.text, r.position as usize).map_err(err)?.0
        };
        png.save(output.join(format!("{}.png", r.item())))
            .map_err(err)?;
    }
    if let Some(new) = report.show
        && index.get(new.item()).is_some()
    {
        std::fs::copy(
            output.join(format!("{}.png", new.item())),
            output.join("display.png"),
        )
        .map_err(err)?;
        let new = Screen {
            job: new.job().into(),
            item: new.item().into(),
            sha: new.sha().into(),
            pending: true,
        };
        save_screen(output, &new)?;
        screen = Some(new);
    }
    // Preserve both the on-screen token and the newest tokens in the logical cache.
    let on_screen = screen.as_ref().map(|r| r.item.as_str()).unwrap_or("");
    index.evict_tokens(&mut s, on_screen, 20).map_err(err)?;
    remove_stale(output, &index)?;
    report
        .result
        .map(|()| report.received)
        .map_err(|e| format!("{} ({e:?})", e.message()))
}

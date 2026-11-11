//! The camera facade: one connected camera, kept in sync and reconnected,
//! with live view and new shots delivered as events. The CLI and the desktop
//! app use this type rather than the protocol layers.
//!
//! A supervisor task owns the connection's lifecycle. While connected it runs
//! two or three loops that share the one connection through a mutex:
//!
//! - the event loop re-reads the whole property table (0x9209 with no
//!   parameters, as libgphoto2 polls it) after DevicePropChanged events, at
//!   most every 100 ms, and at least every 250 ms without them, and emits
//!   [`CameraEvent::StateChanged`] when something changed;
//! - the live view loop fetches frames and emits [`CameraEvent::Frame`];
//! - the capture loop, when `auto_receive` is on, saves new shots into the
//!   destination folder, seeing them waiting in the property table the event
//!   loop keeps fresh (0xD215).
//!
//! When any loop finds the connection gone, the supervisor emits
//! [`CameraEvent::Disconnected`], stops the loops and, if reconnecting is on,
//! retries with a growing delay until the camera answers again.
//!
//! PTP/IP ProbeRequest packets on the event socket are answered with a
//! ProbeResponse by the transport, so a camera checking on an idle host
//! keeps the link.

mod loops;
mod state;

use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::capture::{self, Destination, Received};
use crate::card::{self, CardObject, PartialDownload, CARD_CHUNK};
use crate::error::Error;
use crate::liveview::{self, LiveViewFrame};
use crate::ptp::codes::{
    CTRL_AF_AREA_POSITION, OP_GET_DEVICE_INFO, PROP_FOCUS_AREA, PROP_FOCUS_MODE, PROP_ISO,
};
use crate::ptp::{parse_device_info, PtpValue};
use crate::sony::property::{read_all_props, Enabled, Form, PropertyDesc};
use crate::sony::{control, sdio_connect};
use crate::ssh::{SshConfig, SshTunnel};
use crate::{quirks_for, ConnectOptions, PtpEvent, PtpIpConnection};

use state::Snapshot;

/// How many events a slow subscriber may fall behind before it misses some.
const EVENT_CHANNEL_CAPACITY: usize = 256;
/// Focus mode value for manual focus; any other mode focuses before a shot.
const FOCUS_MODE_MANUAL: PtpValue = PtpValue::U16(1);

/// A boxed future, as returned by a [`ReconnectHook`].
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn std::future::Future<Output = T> + Send + 'a>>;

/// Called with the attempt number before each reconnect attempt from the
/// second on, e.g. to rejoin the camera's Wi-Fi network. The attempt waits
/// for it to finish.
pub type ReconnectHook = Arc<dyn Fn(u32) -> BoxFuture<'static, ()> + Send + Sync>;

/// How to reach and drive a camera.
#[derive(Clone, Serialize, Deserialize)]
pub struct CameraConfig {
    pub addr: SocketAddr,
    /// This host's identity; keep it stable (see [`crate::load_or_create_guid`])
    /// so the camera remembers the pairing.
    pub guid: [u8; 16],
    /// Shown on the camera's pairing prompt, e.g. "Rimix on studio-mac".
    pub host_name: String,
    /// Where new shots are saved.
    pub destination: Destination,
    /// Fetch new shots from the camera as they are taken.
    pub auto_receive: bool,
    /// Reconnect after the link drops.
    pub reconnect: bool,
    /// Runs before reconnect attempts 2 and later (see [`ReconnectHook`]).
    #[serde(skip)]
    pub before_reconnect: Option<ReconnectHook>,
    /// For cameras with Access Authentication: reach PTP/IP through the
    /// camera's SSH server, forwarding to `addr`'s port.
    #[serde(default)]
    pub ssh: Option<SshConfig>,
}

impl std::fmt::Debug for CameraConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CameraConfig")
            .field("addr", &self.addr)
            .field("guid", &self.guid)
            .field("host_name", &self.host_name)
            .field("destination", &self.destination)
            .field("auto_receive", &self.auto_receive)
            .field("reconnect", &self.reconnect)
            .field(
                "before_reconnect",
                &format_args!(
                    "{}",
                    if self.before_reconnect.is_some() {
                        "<set>"
                    } else {
                        "<none>"
                    }
                ),
            )
            .field("ssh", &self.ssh)
            .finish()
    }
}

/// A snapshot of the camera for the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CameraState {
    pub connected: bool,
    /// True while the session runs through the camera's SSH tunnel.
    pub secured: bool,
    pub model: String,
    pub firmware: String,
    /// The camera generation, "3" or "4", from the model table.
    pub generation: String,
    /// Battery level in percent, when the camera reports one.
    pub battery: Option<u8>,
    pub recording: bool,
    pub focus_found: bool,
    /// Every property, in the camera's order.
    pub properties: Vec<PropertySummary>,
    /// Live view frames received per second, over the last second.
    pub live_view_fps: f32,
    /// When the last picture was taken from Rimix or received, in
    /// milliseconds since the Unix epoch.
    pub last_shot_at: Option<u64>,
    /// Whether the camera hands new pictures to the computer: `false` while
    /// its save destination is the card only, `None` when it does not say.
    #[serde(default)]
    pub shots_to_computer: Option<bool>,
    /// The control codes the camera said it accepts when Rimix connected,
    /// such as 0xD2DC, the focus point. For the camera report.
    #[serde(default)]
    pub controls: Vec<u16>,
}

/// One property, ready to show.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PropertySummary {
    pub code: u16,
    pub label: String,
    /// Compact form for chips, e.g. "1600" where `label` is "ISO 1600".
    pub short: String,
    pub raw: PtpValue,
    pub writable: bool,
    pub enabled: Enabled,
    /// The values the camera accepts, in its order. Empty for range-form
    /// properties; use [`Camera::step`] for those.
    pub choices: Vec<Choice>,
    /// The ends and step of a range-form property, such as the colour
    /// temperature in kelvin.
    #[serde(default)]
    pub range: Option<ValueRange>,
}

/// The values a range-form property takes: `min`, `min + step`, ... `max`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValueRange {
    pub min: i64,
    pub max: i64,
    pub step: i64,
}

/// One settable value of a property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub raw: PtpValue,
    pub label: String,
    /// The compact form, as [`PropertySummary::short`]: "P" where `label`
    /// is "Program Auto".
    #[serde(default)]
    pub short: String,
}

/// What happened, for subscribers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CameraEvent {
    Connected,
    Disconnected {
        reason: String,
    },
    /// A reconnect attempt is starting; `attempt` counts from 1.
    Reconnecting {
        attempt: u32,
    },
    StateChanged(CameraState),
    /// A new live view frame is available from [`Camera::latest_frame`].
    Frame,
    CaptureStarted {
        filename: String,
    },
    CaptureProgress {
        filename: String,
        received: u64,
        total: u64,
    },
    CaptureSaved(Received),
    /// The camera offered a shot already saved, byte for byte; the repeat
    /// was downloaded and deleted.
    CaptureSkipped {
        filename: String,
    },
    CaptureFailed {
        filename: String,
        error: String,
    },
    /// A chunk of a file downloaded from the card with
    /// [`Camera::card_download`] arrived; `filename` is the camera's name.
    CardDownloadProgress {
        handle: u32,
        filename: String,
        received: u64,
        total: u64,
    },
    /// A download from the card with [`Camera::card_download`] failed;
    /// `filename` is the camera's name (or `object 0x…` when the object
    /// could not be looked up). Kept apart from [`CameraEvent::CaptureFailed`]
    /// so it never reads as a problem with saving shots.
    CardDownloadFailed {
        handle: u32,
        filename: String,
        error: String,
    },
    /// A movie was recorded but the camera will not let its card be read
    /// in this session, so the clip stays on the card. Sent once per
    /// session, with the camera's refusal as `reason`.
    MovieTransferUnavailable {
        reason: String,
    },
}

/// A connected camera. Dropping it or calling [`Camera::disconnect`] stops
/// every task it started.
pub struct Camera {
    inner: Arc<Inner>,
    supervisor: StdMutex<Option<JoinHandle<()>>>,
}

/// State shared by the facade and its tasks.
struct Inner {
    addr: SocketAddr,
    guid: [u8; 16],
    host_name: String,
    /// What to do about new pictures and movies: see [`Receive`]. Changed
    /// while connected by [`Camera::set_receiving`].
    receive: AtomicU8,
    reconnect: bool,
    before_reconnect: Option<ReconnectHook>,
    ssh: Option<SshConfig>,
    /// The SSH tunnel the current session runs through; dropped with it.
    tunnel: StdMutex<Option<SshTunnel>>,
    destination: StdMutex<Destination>,
    /// `None` while disconnected. Hold it only for one operation at a time.
    conn: tokio::sync::Mutex<Option<PtpIpConnection>>,
    snapshot: StdMutex<Snapshot>,
    frame: StdMutex<Option<Arc<LiveViewFrame>>>,
    events: broadcast::Sender<CameraEvent>,
    /// Set by [`Camera::disconnect`] and on drop; the supervisor then stops.
    stop: AtomicBool,
    /// The last shots saved, kept across reconnects so a shot the camera
    /// offers again is not saved twice.
    recent: StdMutex<VecDeque<RecentShot>>,
    /// The last delivery failure reported, as (camera file name, error
    /// text); the same failure again is not reported. Cleared by a save.
    last_failure: StdMutex<Option<(String, String)>>,
    /// Set when the camera announces (ObjectAdded) a picture in its memory
    /// for the computer; the capture loop takes it as "one is waiting".
    object_added: AtomicBool,
}

/// What a session does about new pictures and movies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Receive {
    /// Nothing: the camera's save setting is not touched and nothing is
    /// fetched (`auto_receive: false`, as the command line tool connects).
    Leave = 0,
    /// Copy them to this computer: the camera is asked to save to computer
    /// and card, and what it offers is fetched.
    Here = 1,
    /// Keep them on the card: the camera is put back to saving on its card
    /// alone and nothing is fetched.
    CardOnly = 2,
}

/// A saved shot, remembered for spotting a repeated delivery.
struct RecentShot {
    /// The camera's file name, before any `-N` suffix.
    filename: String,
    /// Size from the camera's ObjectInfo.
    size: u64,
    /// Where it was saved.
    path: PathBuf,
}

/// What [`connect_once`] sets up.
struct Session {
    conn: PtpIpConnection,
    /// Set when the connection runs through SSH.
    tunnel: Option<SshTunnel>,
    events: broadcast::Receiver<PtpEvent>,
    model: String,
    firmware: String,
    props: Vec<PropertyDesc>,
    /// The control codes the camera said it accepts (0x9202).
    controls: Vec<u16>,
}

/// Locks a std mutex, carrying on if a panicking holder poisoned it.
fn lock<T>(m: &StdMutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl Inner {
    fn emit(&self, event: CameraEvent) {
        // No subscribers is fine.
        let _ = self.events.send(event);
    }

    fn emit_state(&self) {
        let state = lock(&self.snapshot).to_state();
        self.emit(CameraEvent::StateChanged(state));
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn receive(&self) -> Receive {
        match self.receive.load(Ordering::SeqCst) {
            1 => Receive::Here,
            2 => Receive::CardOnly,
            _ => Receive::Leave,
        }
    }

    /// Reads the whole property table and merges it into the snapshot,
    /// emitting StateChanged if anything changed. Returns whether it did.
    async fn refresh(&self, conn: &mut PtpIpConnection) -> Result<bool, Error> {
        let props = read_all_props(conn).await?;
        let changed = lock(&self.snapshot).merge(props);
        if changed {
            self.emit_state();
        }
        Ok(changed)
    }

    fn mark_shot(&self) {
        lock(&self.snapshot).last_shot_at = Some(now_millis());
    }

    /// Stores a fresh session's connection and facts; returns what the loops
    /// need.
    async fn install(&self, session: Session) -> broadcast::Receiver<PtpEvent> {
        *self.conn.lock().await = Some(session.conn);
        let secured = session.tunnel.is_some();
        *lock(&self.tunnel) = session.tunnel;
        let mut snap = lock(&self.snapshot);
        snap.connected = true;
        snap.secured = secured;
        snap.model = session.model;
        snap.firmware = session.firmware;
        snap.props = session.props;
        snap.controls = session.controls;
        snap.live_view_fps = 0.0;
        session.events
    }

    /// Every file on the camera's cards: see [`Camera::card_list`].
    async fn card_list(&self) -> Result<Vec<CardObject>, Error> {
        let storages = {
            let mut guard = self.conn.lock().await;
            card::list_storages(guard.as_mut().ok_or(Error::Disconnected)?).await?
        };
        let mut out = Vec::new();
        for storage in storages {
            let mut walk = card::CardWalk::new(storage);
            while !walk.done() {
                tokio::task::yield_now().await;
                let mut guard = self.conn.lock().await;
                walk.step(guard.as_mut().ok_or(Error::Disconnected)?)
                    .await?;
            }
            out.extend(walk.into_files());
        }
        Ok(out)
    }

    /// Downloads the card file `obj` into the destination folder a chunk at
    /// a time, resuming a partial file. Progress goes out as
    /// [`CameraEvent::CardDownloadProgress`], or as
    /// [`CameraEvent::CaptureProgress`] when `as_capture` (a file Rimix
    /// fetches by itself, shown like a shot arriving).
    async fn card_fetch(&self, obj: &CardObject, as_capture: bool) -> Result<Received, Error> {
        let dest = lock(&self.destination).clone();
        let dir = dest.ensure_writable(chrono::Local::now().date_naive())?;
        let mut dl = PartialDownload::begin(obj, &dir)?;
        while !dl.done() {
            {
                let mut guard = self.conn.lock().await;
                let conn = guard.as_mut().ok_or(Error::Disconnected)?;
                dl.next_chunk(conn, CARD_CHUNK).await?;
            }
            let (filename, received, total) = (obj.filename.clone(), dl.received(), dl.total());
            self.emit(if as_capture {
                CameraEvent::CaptureProgress {
                    filename,
                    received,
                    total,
                }
            } else {
                CameraEvent::CardDownloadProgress {
                    handle: obj.handle,
                    filename,
                    received,
                    total,
                }
            });
            tokio::task::yield_now().await;
        }
        let path = dl.finish()?;
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Received {
            path,
            bytes: obj.bytes,
            filename,
            camera_filename: obj.filename.clone(),
        })
    }

    /// Drops the connection and tells subscribers.
    async fn lose_connection(&self, reason: String) {
        self.conn.lock().await.take();
        lock(&self.tunnel).take();
        {
            let mut snap = lock(&self.snapshot);
            snap.connected = false;
            snap.secured = false;
            snap.live_view_fps = 0.0;
        }
        self.emit(CameraEvent::Disconnected { reason });
        self.emit_state();
    }
}

/// Opens the connection and reads what the facade needs: transport connect,
/// GetDeviceInfo, the SDIO handshake, every property, and a live view check.
/// With SSH configured, a fresh tunnel is opened first and PTP/IP dials it.
async fn connect_once(inner: &Inner) -> Result<Session, Error> {
    let opts = ConnectOptions {
        guid: inner.guid,
        name: inner.host_name.clone(),
        ..ConnectOptions::default()
    };
    let tunnel = match &inner.ssh {
        Some(ssh) => {
            let camera = SocketAddr::new(inner.addr.ip(), ssh.port);
            Some(SshTunnel::open(camera, ssh, inner.addr.port()).await?)
        }
        None => None,
    };
    let target = tunnel.as_ref().map_or(inner.addr, SshTunnel::local_addr);
    let (mut conn, _) = PtpIpConnection::connect(target, opts).await?;
    // Subscribe before reading properties so no change is missed.
    let events = conn.events();
    let (data, _) = conn.command_data_in(OP_GET_DEVICE_INFO, &[]).await?;
    let info = parse_device_info(&data)?;
    let ext = sdio_connect(&mut conn).await?;
    let mut props = read_all_props(&mut conn).await?;
    // A camera saving to its card only never offers a shot to the computer.
    if inner.receive() == Receive::Here
        && capture::ensure_host_delivery(&mut conn, &props, &quirks_for(&info.model)).await?
    {
        props = read_all_props(&mut conn).await?;
    }
    // Live view never fails the connection: a camera that refuses the check
    // (live view still starting, busy, or off) is polled anyway, and
    // `next_frame` waits quietly until frames come. Only a transport failure
    // here ends the attempt.
    match liveview::start(&mut conn).await {
        Ok(()) => {}
        Err(e @ (Error::Unsupported(_) | Error::Response { .. })) => {
            tracing::info!(error = %e, "live view not ready yet; polling anyway");
        }
        Err(e) => return Err(e),
    }
    Ok(Session {
        conn,
        tunnel,
        events,
        model: info.model,
        firmware: info.device_version,
        props,
        controls: ext.controls,
    })
}

impl Camera {
    /// True when a [`ReconnectHook`] runs before reconnect attempts.
    pub fn has_reconnect_hook(&self) -> bool {
        self.inner.before_reconnect.is_some()
    }

    /// Connects to the camera and starts the event, live view and capture
    /// loops. Fails if this first connection fails; reconnecting applies only
    /// after a connection has been made.
    pub async fn connect(cfg: CameraConfig) -> Result<Camera, Error> {
        let inner = Arc::new(Inner {
            addr: cfg.addr,
            guid: cfg.guid,
            host_name: cfg.host_name,
            receive: AtomicU8::new(if cfg.auto_receive {
                Receive::Here as u8
            } else {
                Receive::Leave as u8
            }),
            reconnect: cfg.reconnect,
            before_reconnect: cfg.before_reconnect,
            ssh: cfg.ssh,
            tunnel: StdMutex::new(None),
            destination: StdMutex::new(cfg.destination),
            conn: tokio::sync::Mutex::new(None),
            snapshot: StdMutex::new(Snapshot::default()),
            frame: StdMutex::new(None),
            events: broadcast::channel(EVENT_CHANNEL_CAPACITY).0,
            stop: AtomicBool::new(false),
            recent: StdMutex::new(VecDeque::new()),
            last_failure: StdMutex::new(None),
            object_added: AtomicBool::new(false),
        });
        let session = connect_once(&inner).await?;
        let events = inner.install(session).await;
        inner.emit(CameraEvent::Connected);
        let supervisor = tokio::spawn(loops::supervise(inner.clone(), events));
        Ok(Camera {
            inner,
            supervisor: StdMutex::new(Some(supervisor)),
        })
    }

    /// Subscribes to events. Events sent before subscribing are not seen.
    pub fn subscribe(&self) -> broadcast::Receiver<CameraEvent> {
        self.inner.events.subscribe()
    }

    pub fn state(&self) -> CameraState {
        lock(&self.inner.snapshot).to_state()
    }

    pub fn latest_frame(&self) -> Option<Arc<LiveViewFrame>> {
        lock(&self.inner.frame).clone()
    }

    /// Sets a property to `value`, which must be one of its settable values
    /// (or on its range), then refreshes the state, emitting StateChanged
    /// if the camera now reports anything different.
    pub async fn set(&self, code: u16, value: PtpValue) -> Result<(), Error> {
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        let (desc, quirks) = {
            let snap = lock(&self.inner.snapshot);
            let desc = snap.prop(code).cloned();
            (desc, quirks_for(&snap.model))
        };
        let desc = desc.ok_or(Error::Unsupported("the camera has no such property"))?;
        if matches!(desc.form, Form::None) {
            // No list of values: the camera only takes nudges (shutter
            // speed and aperture on the ZV-E10), so it is walked there.
            if !desc.writable || desc.enabled == Enabled::Off {
                return Err(Error::Unsupported("property cannot be set now"));
            }
            control::nudge_to(conn, &desc, &value).await?;
        } else {
            if !desc.writable || desc.enabled != Enabled::On {
                return Err(Error::Unsupported("property cannot be set now"));
            }
            control::set_value(conn, &desc, &value, &quirks).await?;
        }
        // The camera took the write; a failed read-back is not a failed
        // write unless the link is gone. The event loop reads again anyway.
        match self.inner.refresh(conn).await {
            Ok(_) => {}
            Err(e) if matches!(e, Error::Disconnected) || conn.is_disconnected() => return Err(e),
            Err(e) => tracing::warn!(error = %e, "reading the settings back after a write failed"),
        }
        Ok(())
    }

    /// Moves a property `delta` places through its settable values (or
    /// `delta` steps through its range), stopping at the ends.
    pub async fn step(&self, code: u16, delta: i32) -> Result<(), Error> {
        let desc = lock(&self.inner.snapshot).prop(code).cloned();
        let desc = desc.ok_or(Error::Unsupported("the camera has no such property"))?;
        if matches!(desc.form, Form::None) {
            return self.nudge(&desc, delta).await;
        }
        let target = if code == PROP_ISO {
            state::iso_step_target(&desc, delta)?
        } else {
            state::step_target(&desc, delta)?
        };
        if target == desc.current {
            return Ok(());
        }
        self.set(code, target).await
    }

    /// Moves a property that lists no values `delta` steps, one nudge at a
    /// time, then reads the settings back.
    async fn nudge(&self, desc: &PropertyDesc, delta: i32) -> Result<(), Error> {
        if !desc.writable || desc.enabled == Enabled::Off {
            return Err(Error::Unsupported("property cannot be set now"));
        }
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        let direction = PtpValue::I8(if delta > 0 { 1 } else { -1 });
        for _ in 0..delta.unsigned_abs().min(control::MAX_STEPS as u32) {
            control::control(conn, desc.code, &direction).await?;
        }
        match self.inner.refresh(conn).await {
            Ok(_) => Ok(()),
            Err(e) if matches!(e, Error::Disconnected) || conn.is_disconnected() => Err(e),
            Err(e) => {
                tracing::warn!(error = %e, "reading the settings back after a step failed");
                Ok(())
            }
        }
    }

    /// Takes a picture, focusing first unless the camera reports manual focus.
    pub async fn shoot(&self) -> Result<(), Error> {
        let with_af = lock(&self.inner.snapshot)
            .prop(PROP_FOCUS_MODE)
            .is_none_or(|p| p.current != FOCUS_MODE_MANUAL);
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        control::shoot(conn, with_af).await?;
        drop(guard);
        self.inner.mark_shot();
        self.inner.emit_state();
        Ok(())
    }

    /// Focuses at a point of the picture, as a tap on the camera's own
    /// screen does. `x` and `y` run from 0 to 1 across and down the picture.
    ///
    /// The point only means something while autofocus uses a movable spot,
    /// so the focus area is switched to Flexible Spot: M first when it is
    /// something else and the camera lists that. Then the point is sent
    /// (0xD2DC, on the camera's 640 by 480 grid, as libgphoto2's
    /// `_put_Sony_SpotFocusArea`) and the shutter is half pressed to focus.
    pub async fn focus_at(&self, x: f32, y: f32) -> Result<(), Error> {
        /// The grid the camera counts the point on.
        const GRID: (f32, f32) = (640.0, 480.0);
        /// Flexible Spot: M.
        const SPOT: u16 = 0x0102;
        let on_grid = |v: f32, size: f32| (v.clamp(0.0, 1.0) * size).min(size - 1.0) as u32;
        let point = (on_grid(x, GRID.0) << 16) | on_grid(y, GRID.1);
        let (area, quirks) = {
            let snap = lock(&self.inner.snapshot);
            (snap.prop(PROP_FOCUS_AREA).cloned(), quirks_for(&snap.model))
        };
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        if let Some(area) = area {
            // 0x01xx are the movable spots, 0x02xx their tracking forms
            // (0x0204 and up); wide, zone and centre ignore a point.
            let movable = area
                .current
                .as_u64()
                .is_some_and(|v| (0x0101..=0x0104).contains(&v) || (0x0204..=0x0207).contains(&v));
            let spot = PtpValue::U16(SPOT);
            if !movable && area.writable && area.enabled == Enabled::On && area.accepts(&spot) {
                match control::set_value(conn, &area, &spot, &quirks).await {
                    Ok(()) => {
                        tracing::info!("focus area set to Flexible Spot: M for a focus point")
                    }
                    Err(e) if matches!(e, Error::Disconnected) || conn.is_disconnected() => {
                        return Err(e)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "the camera refused Flexible Spot; sending the point anyway")
                    }
                }
            }
        }
        tracing::info!(x, y, point = %format_args!("{point:#010x}"), "focus point");
        control::control(conn, CTRL_AF_AREA_POSITION, &PtpValue::U32(point)).await?;
        control::focus(conn).await
    }

    /// Half presses the shutter long enough to focus.
    pub async fn focus(&self) -> Result<(), Error> {
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        control::focus(conn).await
    }

    /// Starts or stops movie recording.
    pub async fn toggle_record(&self) -> Result<(), Error> {
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        control::toggle_record(conn).await
    }

    /// Every file on the camera's cards, storage by storage, walking each
    /// card's folders. The connection
    /// is held for one PTP operation at a time, so live view keeps going.
    pub async fn card_list(&self) -> Result<Vec<CardObject>, Error> {
        self.inner.card_list().await
    }

    /// Downloads the card file `handle` into the destination folder,
    /// resuming a partial file an earlier try left there. Emits
    /// [`CameraEvent::CardDownloadProgress`] after every chunk and
    /// [`CameraEvent::CaptureSaved`] at the end, or
    /// [`CameraEvent::CardDownloadFailed`] on error. The connection is held for
    /// one chunk at a time, so live view keeps going.
    pub async fn card_download(&self, handle: u32) -> Result<PathBuf, Error> {
        let looked_up = async {
            let mut guard = self.inner.conn.lock().await;
            let conn = guard.as_mut().ok_or(Error::Disconnected)?;
            card::object(conn, handle)
                .await?
                .ok_or(Error::Unsupported("that card object is a folder"))
        }
        .await;
        let obj =
            looked_up.map_err(|e| self.card_failed(handle, format!("object {handle:#x}"), e))?;
        match self.inner.card_fetch(&obj, false).await {
            Ok(saved) => {
                let path = saved.path.clone();
                self.inner.emit(CameraEvent::CaptureSaved(saved));
                Ok(path)
            }
            Err(e) => Err(self.card_failed(handle, obj.filename, e)),
        }
    }

    /// Emits [`CameraEvent::CardDownloadFailed`] for a card download;
    /// returns `e`.
    fn card_failed(&self, handle: u32, filename: String, e: Error) -> Error {
        tracing::warn!(file = %filename, error = %e, "card download failed");
        self.inner.emit(CameraEvent::CardDownloadFailed {
            handle,
            filename,
            error: e.to_string(),
        });
        e
    }

    /// Turns copying new pictures and movies to this computer on or off
    /// while connected. On, the camera is asked to save to computer and card
    /// and what it then offers is fetched. Off, it is put back to saving on
    /// its card alone, so it never waits for a computer that will not fetch.
    /// The change reaches the camera within a second or two; watch
    /// [`CameraState::shots_to_computer`].
    pub fn set_receiving(&self, on: bool) {
        let to = if on { Receive::Here } else { Receive::CardOnly };
        self.inner.receive.store(to as u8, Ordering::SeqCst);
    }

    /// Changes where later shots are saved.
    pub fn set_destination(&self, dest: Destination) {
        *lock(&self.inner.destination) = dest;
    }

    /// Stops every task, closes the session and does not reconnect.
    pub async fn disconnect(&self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        let supervisor = lock(&self.supervisor).take();
        if let Some(task) = supervisor {
            task.abort();
            // Waiting makes sure no loop still holds or replaces the connection.
            let _ = task.await;
        }
        let conn = self.inner.conn.lock().await.take();
        if let Some(conn) = conn {
            if !conn.is_disconnected() {
                if let Err(e) = conn.close().await {
                    tracing::debug!(error = %e, "closing the session failed");
                }
            }
        }
        // After closing, which goes through the tunnel.
        lock(&self.inner.tunnel).take();
        lock(&self.inner.snapshot).secured = false;
        let was_connected = std::mem::replace(&mut lock(&self.inner.snapshot).connected, false);
        if was_connected {
            self.inner.emit(CameraEvent::Disconnected {
                reason: "disconnected".into(),
            });
            self.inner.emit_state();
        }
    }
}

impl Drop for Camera {
    fn drop(&mut self) {
        self.inner.stop.store(true, Ordering::SeqCst);
        if let Some(task) = lock(&self.supervisor).take() {
            // Aborting the supervisor drops its loops with it.
            task.abort();
        }
    }
}

impl std::fmt::Debug for Camera {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Camera")
            .field("addr", &self.inner.addr)
            .field("connected", &lock(&self.inner.snapshot).connected)
            .finish_non_exhaustive()
    }
}

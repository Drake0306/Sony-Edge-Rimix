//! The camera facade: one connected camera, kept in sync and reconnected,
//! with live view and new shots delivered as events. The CLI and the desktop
//! app use this type rather than the protocol layers.
//!
//! A supervisor task owns the connection's lifecycle. While connected it runs
//! two or three loops that share the one connection through a mutex:
//!
//! - the event loop re-reads changed properties after DevicePropChanged
//!   events (at most every 100 ms) and emits [`CameraEvent::StateChanged`];
//! - the live view loop fetches frames and emits [`CameraEvent::Frame`];
//! - the capture loop, when `auto_receive` is on, saves new shots into the
//!   destination folder.
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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::capture::{Destination, Received};
use crate::error::Error;
use crate::liveview::{self, LiveViewFrame};
use crate::ptp::codes::{OP_GET_DEVICE_INFO, PROP_FOCUS_MODE};
use crate::ptp::{parse_device_info, PtpValue};
use crate::sony::property::{read_all_props, read_changed_props, Enabled, PropertyDesc};
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
}

/// One settable value of a property.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Choice {
    pub raw: PtpValue,
    pub label: String,
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
    auto_receive: bool,
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

    fn merge(&self, changed: Vec<PropertyDesc>) {
        lock(&self.snapshot).merge(changed);
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
        snap.live_view_fps = 0.0;
        session.events
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
    sdio_connect(&mut conn).await?;
    let props = read_all_props(&mut conn).await?;
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
            auto_receive: cfg.auto_receive,
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
    /// (or on its range), then refreshes the state and emits StateChanged.
    pub async fn set(&self, code: u16, value: PtpValue) -> Result<(), Error> {
        let mut guard = self.inner.conn.lock().await;
        let conn = guard.as_mut().ok_or(Error::Disconnected)?;
        let (desc, quirks) = {
            let snap = lock(&self.inner.snapshot);
            let desc = snap.prop(code).cloned();
            (desc, quirks_for(&snap.model))
        };
        let desc = desc.ok_or(Error::Unsupported("the camera has no such property"))?;
        if !desc.writable || desc.enabled != Enabled::On {
            return Err(Error::Unsupported("property cannot be set now"));
        }
        control::set_value(conn, &desc, &value, &quirks).await?;
        let changed = read_changed_props(conn).await?;
        drop(guard);
        self.inner.merge(changed);
        self.inner.emit_state();
        Ok(())
    }

    /// Moves a property `delta` places through its settable values (or
    /// `delta` steps through its range), stopping at the ends.
    pub async fn step(&self, code: u16, delta: i32) -> Result<(), Error> {
        let desc = lock(&self.inner.snapshot).prop(code).cloned();
        let desc = desc.ok_or(Error::Unsupported("the camera has no such property"))?;
        let target = state::step_target(&desc, delta)?;
        if target == desc.current {
            return Ok(());
        }
        self.set(code, target).await
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

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::{encode_device_info, DeviceInfo, PtpValue};
use sony_ptpip::ptpip::{read_frame, Packet};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use crate::card;
use crate::controls::{Controls, Outcome};
use crate::frames::{self, LiveView};
use crate::props::{load_fixture, PropStore};
use crate::{ssdp, ssh};
use crate::{SimCommand, SimConfig, SimHandle};

const PROTOCOL_VERSION: u32 = 0x0001_0000;
const SIM_GUID: [u8; 16] = *b"rimix-simulator!";
const DEVICE_VERSION: &str = "2.00";
const VENDOR_EXT_ID: u32 = 0x11;
/// How long a command connection waits for its event connection.
const EVENT_SOCKET_WAIT: Duration = Duration::from_secs(10);
const EXT_CONTROLS: [u16; 3] = [0xD2C1, 0xD2C2, 0xD2C8];
const EXT_VERSION: u16 = 0x012C;
/// How many Data packets a paced capture is split into.
const PACED_PACKETS: usize = 5;
/// How long a held half press takes to find focus.
const FOCUS_DELAY: Duration = Duration::from_millis(200);
/// Largest Data packet of card data.
const CARD_PACKET: usize = 256 * 1024;
const SUPPORTED_OPS: [u16; 14] = [
    OP_GET_DEVICE_INFO,
    OP_OPEN_SESSION,
    OP_CLOSE_SESSION,
    OP_GET_STORAGE_IDS,
    OP_GET_OBJECT_HANDLES,
    OP_GET_OBJECT_INFO,
    OP_GET_OBJECT,
    OP_GET_PARTIAL_OBJECT,
    OP_SDIO_CONNECT,
    OP_SDIO_GET_EXT_DEVICE_INFO,
    OP_SDIO_SET_EXT_DEVICE_PROP_VALUE,
    OP_SDIO_CONTROL_DEVICE,
    OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO,
    OP_SDIO_GET_EXT_DEVICE_PROP,
];

type Log = Arc<Mutex<Vec<String>>>;

/// State shared by the listener and its connection tasks.
struct Shared {
    config: SimConfig,
    log: Log,
    next_conn: AtomicU32,
    /// Command connections waiting for their event socket, by conn_number.
    pending: Mutex<HashMap<u32, oneshot::Sender<TcpStream>>>,
    /// Control channel of the most recent session.
    current: Mutex<Option<mpsc::Sender<SimCommand>>>,
    /// Camera properties, kept across sessions like a real camera's settings.
    props: Mutex<PropStore>,
    /// Button state and the capture queue; lock before `props` when both are needed.
    controls: Mutex<Controls>,
    live_view: Mutex<LiveView>,
    /// GetObjectInfo requests on the live view handle so far.
    live_view_infos: AtomicU32,
    /// 0x9207 controls received so far.
    controls_seen: AtomicU32,
    /// The files on the card: `config.card`, plus a clip for every movie
    /// recording stopped since.
    card: Mutex<Vec<card::SimCardFile>>,
    /// Bytes of card data sent so far.
    card_sent: AtomicU64,
    /// Set once `drop_during_card_download_at` has dropped the link.
    card_dropped: AtomicBool,
}

/// Starts the simulator on `config.bind_addr`, by default 127.0.0.1 with an
/// OS-chosen port.
///
/// Panics if the property fixture cannot be loaded.
pub async fn spawn(config: SimConfig) -> SimHandle {
    let mut props = load_fixture(&config.fixture).expect("load simulator property fixture");
    let overrides = [
        (
            PROP_STILL_DESTINATION,
            config.still_destination.map(PtpValue::U16),
        ),
        (
            PROP_IMAGE_SIZE_TO_HOST,
            config.host_image_size.map(PtpValue::U8),
        ),
    ];
    for (code, value) in overrides {
        if let (Some(value), Some(prop)) = (value, props.iter_mut().find(|p| p.code == code)) {
            prop.current = value;
        }
    }
    let bind_addr = config
        .bind_addr
        .unwrap_or_else(|| SocketAddr::from(([127, 0, 0, 1], 0)));
    let listener = TcpListener::bind(bind_addr)
        .await
        .expect("bind simulator listener");
    let addr = listener.local_addr().expect("listener address");
    let log: Log = Arc::default();
    let controls = Controls::new(config.files_per_shot);
    let shared = Arc::new(Shared {
        card: Mutex::new(config.card.clone()),
        config,
        log: log.clone(),
        next_conn: AtomicU32::new(1),
        pending: Mutex::default(),
        current: Mutex::default(),
        props: Mutex::new(PropStore::new(props)),
        controls: Mutex::new(controls),
        live_view: Mutex::default(),
        live_view_infos: AtomicU32::new(0),
        controls_seen: AtomicU32::new(0),
        card_sent: AtomicU64::new(0),
        card_dropped: AtomicBool::new(false),
    });
    // Dropping the set when `run` exits aborts every connection, session and
    // discovery task.
    let mut tasks = JoinSet::new();
    let (ssdp_addr, http_addr) = match shared.config.ssdp_port {
        Some(port) => {
            let (ssdp, http) = ssdp::start(port, shared.config.ssh.is_some(), &mut tasks).await;
            (Some(ssdp), Some(http))
        }
        None => (None, None),
    };
    let ssh_account = shared
        .config
        .ssh
        .clone()
        .map(|account| Arc::new(Mutex::new(account)));
    let ssh = match ssh_account.clone() {
        Some(account) => Some(
            ssh::start(
                account,
                shared.config.ssh_reject_host.clone(),
                log.clone(),
                addr.ip(),
                addr,
                &mut tasks,
            )
            .await,
        ),
        None => None,
    };
    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(run(listener, shared, rx, tasks, ssh_account));
    SimHandle {
        addr,
        events: tx,
        log,
        http_addr,
        ssdp_addr,
        ssh_addr: ssh.as_ref().map(|s| s.addr),
        fingerprint_sha256: ssh.as_ref().map(|s| s.fingerprint_sha256.clone()),
        fingerprint_md5: ssh.map(|s| s.fingerprint_md5),
    }
}

async fn run(
    listener: TcpListener,
    shared: Arc<Shared>,
    mut control: mpsc::Receiver<SimCommand>,
    mut tasks: JoinSet<()>,
    ssh_account: Option<Arc<Mutex<ssh::SshSimConfig>>>,
) {
    loop {
        tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    tasks.spawn(handle_connection(stream, shared.clone()));
                }
                Err(e) => tracing::warn!(error = %e, "simulator accept failed"),
            },
            cmd = control.recv() => match cmd {
                None | Some(SimCommand::Shutdown) => break,
                Some(SimCommand::SetSshPassword(password)) => match &ssh_account {
                    Some(account) => account.lock().unwrap().password = password,
                    None => tracing::debug!("no SSH front to change the password of"),
                },
                Some(cmd) => {
                    let session = shared.current.lock().unwrap().clone();
                    match session {
                        Some(s) => {
                            let _ = s.send(cmd).await;
                        }
                        None => tracing::debug!(?cmd, "no session for simulator command"),
                    }
                }
            },
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
}

async fn recv(r: &mut (impl tokio::io::AsyncRead + Unpin)) -> Result<Packet, Error> {
    Packet::decode(&read_frame(r).await?)
}

async fn send(w: &mut (impl AsyncWriteExt + Unpin), p: &Packet) -> Result<(), Error> {
    w.write_all(&p.encode()).await?;
    Ok(())
}

/// Reads the first packet to decide whether this is a command or event socket.
async fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>) {
    let result = match recv(&mut stream).await {
        Ok(Packet::InitCommandRequest { .. }) => command_connection(stream, shared).await,
        Ok(Packet::InitEventRequest { conn_number }) => {
            event_connection(stream, conn_number, &shared).await
        }
        Ok(other) => Err(Error::Protocol(format!(
            "unexpected first packet {other:?}"
        ))),
        Err(e) => Err(e),
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "simulator connection ended");
    }
}

async fn event_connection(
    mut stream: TcpStream,
    conn_number: u32,
    shared: &Shared,
) -> Result<(), Error> {
    let waiter = shared.pending.lock().unwrap().remove(&conn_number);
    let Some(waiter) = waiter else {
        send(&mut stream, &Packet::InitFail { reason: 1 }).await?;
        return Err(Error::Protocol(format!(
            "unknown conn_number {conn_number}"
        )));
    };
    send(&mut stream, &Packet::InitEventAck).await?;
    let _ = waiter.send(stream);
    Ok(())
}

async fn command_connection(mut stream: TcpStream, shared: Arc<Shared>) -> Result<(), Error> {
    let conn_number = shared.next_conn.fetch_add(1, Ordering::SeqCst);
    let (ev_tx, ev_rx) = oneshot::channel();
    shared.pending.lock().unwrap().insert(conn_number, ev_tx);
    if shared.config.require_pairing {
        tokio::time::sleep(shared.config.pairing_delay).await;
    }
    send(
        &mut stream,
        &Packet::InitCommandAck {
            conn_number,
            guid: SIM_GUID,
            name: shared.config.model.clone(),
            version: PROTOCOL_VERSION,
        },
    )
    .await?;
    let event = tokio::time::timeout(EVENT_SOCKET_WAIT, ev_rx).await;
    let Ok(Ok(event)) = event else {
        shared.pending.lock().unwrap().remove(&conn_number);
        return Err(Error::Protocol("event socket never arrived".into()));
    };

    let (ctl_tx, ctl_rx) = mpsc::channel(16);
    *shared.current.lock().unwrap() = Some(ctl_tx);
    Session {
        shared: shared.clone(),
        stream,
        event,
        handshake_phase: 0,
    }
    .run(ctl_rx)
    .await
}

/// One paired command + event socket.
struct Session {
    shared: Arc<Shared>,
    stream: TcpStream,
    event: TcpStream,
    /// How many SDIO handshake phases (0x9201) have completed, 0..=3.
    handshake_phase: u8,
}

/// Logs every packet the client sends on the event socket.
async fn log_event_packets(mut r: OwnedReadHalf, log: Log) {
    while let Ok(p) = recv(&mut r).await {
        log.lock().unwrap().push(format!("event {p:?}"));
    }
}

/// Forwards command-socket packets to the session loop; `read_frame` is not
/// cancel-safe, so it cannot sit directly in a `select!`.
async fn read_packets(mut r: OwnedReadHalf, tx: mpsc::Sender<Result<Packet, Error>>) {
    loop {
        let p = recv(&mut r).await;
        let stop = p.is_err();
        if tx.send(p).await.is_err() || stop {
            return;
        }
    }
}

impl Session {
    async fn run(self, mut control: mpsc::Receiver<SimCommand>) -> Result<(), Error> {
        let Session {
            shared,
            stream,
            event,
            mut handshake_phase,
        } = self;
        let (r, mut w) = stream.into_split();
        let (event_r, mut event) = event.into_split();
        let event_reader = tokio::spawn(log_event_packets(event_r, shared.log.clone()));
        let (pkt_tx, mut packets) = mpsc::channel(16);
        let reader = tokio::spawn(read_packets(r, pkt_tx));
        // Set once a request is left unanswered: from then on the session
        // reads and drops every packet, like a camera stuck on an operation.
        let mut hung = false;
        let result = loop {
            tokio::select! {
                p = packets.recv() => {
                    let p = match p {
                        Some(Ok(p)) => p,
                        Some(Err(e)) => break Err(e),
                        None => break Ok(()),
                    };
                    if hung {
                        tracing::debug!(?p, "simulator stuck; dropping packet");
                        continue;
                    }
                    let mut ctx = Ctx { packets: &mut packets, w: &mut w, event: &mut event, shared: &shared, hung: &mut hung };
                    if let Err(e) = handle_packet(p, &mut ctx, &mut handshake_phase).await {
                        break Err(e);
                    }
                }
                cmd = control.recv() => match cmd {
                    Some(SimCommand::SetProperty { code, value }) => {
                        let changed = shared.props.lock().unwrap().set(code, value);
                        if !changed {
                            tracing::debug!(code, "simulator property unknown or unchanged");
                        } else if let Err(e) = send_prop_changed(&mut event, code).await {
                            break Err(e);
                        }
                    }
                    Some(SimCommand::Probe) => {
                        if let Err(e) = send(&mut event, &Packet::ProbeRequest).await {
                            break Err(e);
                        }
                    }
                    Some(SimCommand::DropConnection) => {
                        if shared.config.redeliver_after_drop {
                            let mut controls = shared.controls.lock().unwrap();
                            let mut props = shared.props.lock().unwrap();
                            controls.requeue_last(&mut props);
                        }
                        break Ok(());
                    }
                    // Handled by the listener; never routed to a session.
                    Some(SimCommand::SetSshPassword(_)) => {}
                    Some(SimCommand::Shutdown) | None => break Ok(()),
                },
            }
        };
        // Dropping the read half (via abort), the write half and the event
        // stream closes both sockets.
        reader.abort();
        event_reader.abort();
        result
    }
}

async fn next_packet(packets: &mut mpsc::Receiver<Result<Packet, Error>>) -> Result<Packet, Error> {
    packets.recv().await.unwrap_or(Err(Error::Disconnected))
}

/// Reads a data-out phase (StartData, Data*, EndData) and returns its bytes.
async fn read_data_out(
    packets: &mut mpsc::Receiver<Result<Packet, Error>>,
) -> Result<Vec<u8>, Error> {
    match next_packet(packets).await? {
        Packet::StartData { .. } => {}
        other => {
            return Err(Error::Protocol(format!(
                "expected StartData, got {other:?}"
            )))
        }
    }
    let mut data = Vec::new();
    loop {
        match next_packet(packets).await? {
            Packet::Data { payload, .. } => data.extend(payload),
            Packet::EndData { payload, .. } => {
                data.extend(payload);
                return Ok(data);
            }
            other => return Err(Error::Protocol(format!("expected Data, got {other:?}"))),
        }
    }
}

async fn send_data_in(
    w: &mut (impl AsyncWriteExt + Unpin),
    tid: u32,
    data: &[u8],
) -> Result<(), Error> {
    send(
        w,
        &Packet::StartData {
            tid,
            total_len: data.len() as u64,
        },
    )
    .await?;
    // Split into Data + EndData so clients exercise multi-packet assembly.
    let (head, tail) = data.split_at(data.len() / 2);
    send(
        w,
        &Packet::Data {
            tid,
            payload: head.to_vec(),
        },
    )
    .await?;
    send(
        w,
        &Packet::EndData {
            tid,
            payload: tail.to_vec(),
        },
    )
    .await
}

/// Sends `data` as StartData and [`PACED_PACKETS`] packets, pausing before
/// each packet after the first.
async fn send_data_in_paced(
    w: &mut (impl AsyncWriteExt + Unpin),
    tid: u32,
    data: &[u8],
    pause: Duration,
) -> Result<(), Error> {
    let total_len = data.len() as u64;
    send(w, &Packet::StartData { tid, total_len }).await?;
    let chunk = data.len().div_ceil(PACED_PACKETS).max(1);
    let chunks: Vec<&[u8]> = data.chunks(chunk).collect();
    for (i, part) in chunks.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(pause).await;
        }
        let payload = part.to_vec();
        let p = if i + 1 == chunks.len() {
            Packet::EndData { tid, payload }
        } else {
            Packet::Data { tid, payload }
        };
        send(w, &p).await?;
    }
    Ok(())
}

async fn send_prop_changed(
    event: &mut (impl AsyncWriteExt + Unpin),
    code: u16,
) -> Result<(), Error> {
    let ev = Packet::Event {
        code: EV_SONY_DEVICE_PROP_CHANGED,
        tid: 0,
        params: vec![code as u32],
    };
    send(event, &ev).await
}

/// What a command needs from its session.
struct Ctx<'a, W> {
    packets: &'a mut mpsc::Receiver<Result<Packet, Error>>,
    w: &'a mut W,
    event: &'a mut OwnedWriteHalf,
    shared: &'a Shared,
    /// Set by an operation the simulator never answers (see
    /// `SimConfig::props_param_hangs` and `single_prop_read_hangs`).
    hung: &'a mut bool,
}

async fn handle_packet<W: AsyncWriteExt + Unpin>(
    p: Packet,
    ctx: &mut Ctx<'_, W>,
    handshake_phase: &mut u8,
) -> Result<(), Error> {
    let shared = ctx.shared;
    let Packet::CmdRequest {
        data_phase,
        op,
        tid,
        params,
        ..
    } = p
    else {
        tracing::debug!(?p, "simulator ignoring packet");
        return Ok(());
    };
    let mut line = format!("op {op:#06x} tid {tid}");
    let mut data = None;
    if data_phase == 2 {
        let d = read_data_out(ctx.packets).await?;
        line.push_str(&format!(" data-out {}", d.len()));
        data = Some(d);
    }
    if op == OP_SDIO_CONTROL_DEVICE {
        if let (Some(control), Some(value)) = (prop_code(&params), &data) {
            let mut n = 0u64;
            for (i, b) in value.iter().take(8).enumerate() {
                n |= u64::from(*b) << (8 * i);
            }
            line.push_str(&format!(" {control:04X}={n}"));
        }
    }
    if let Some((slow_op, delay)) = shared.config.delay_op {
        if slow_op == op {
            tokio::time::sleep(delay).await;
        }
    }

    let mut changed_prop = None;
    let mut resp_params = Vec::new();
    let card_now = shared.card.lock().unwrap().clone();
    let card_file = params
        .first()
        .and_then(|&h| card::file(&card_now, h).map(|f| (h, f)));
    let code = match op {
        OP_OPEN_SESSION | OP_CLOSE_SESSION => RC_OK,
        OP_GET_DEVICE_INFO => {
            let info = DeviceInfo {
                vendor_ext_id: VENDOR_EXT_ID,
                model: shared.config.model.clone(),
                device_version: DEVICE_VERSION.into(),
                operations: SUPPORTED_OPS.to_vec(),
            };
            send_data_in(ctx.w, tid, &encode_device_info(&info)).await?;
            RC_OK
        }
        OP_GET_STORAGE_IDS if shared.config.card_denied => RC_ACCESS_DENIED,
        OP_GET_STORAGE_IDS => {
            send_data_in(ctx.w, tid, &card::storage_ids()).await?;
            RC_OK
        }
        OP_GET_OBJECT_HANDLES => match params.first() {
            Some(&card::STORAGE_ID) | Some(&0xFFFF_FFFF) => {
                let card = &card_now;
                let parent = params.get(2).copied().unwrap_or(card::ANY_PARENT);
                line.push_str(&format!(" parent={parent:#x}"));
                match card::children(card, parent) {
                    Some(handles) => {
                        let names: Vec<String> =
                            handles.iter().map(|&h| card::name(card, h)).collect();
                        line.push_str(&format!(" returned={}", names.join(",")));
                        send_data_in(ctx.w, tid, &card::handles(&handles)).await?;
                        RC_OK
                    }
                    None => RC_INVALID_OBJECT_HANDLE,
                }
            }
            _ => RC_INVALID_PARAMETER,
        },
        OP_GET_OBJECT_INFO
            if card_file.is_some_and(|(_, f)| {
                shared.config.card_info_fails_for.as_deref() == Some(f.filename.as_str())
            }) =>
        {
            RC_ACCESS_DENIED
        }
        OP_GET_OBJECT_INFO if card_file.is_some() => {
            let (_, f) = card_file.expect("checked");
            send_data_in(ctx.w, tid, &card::object_info(f)).await?;
            RC_OK
        }
        OP_GET_OBJECT_INFO
            if params
                .first()
                .is_some_and(|&h| card::folder(&card_now, h).is_some()) =>
        {
            let path = card::folder(&card_now, params[0]).expect("checked");
            send_data_in(ctx.w, tid, &card::folder_info(&path)).await?;
            RC_OK
        }
        OP_GET_OBJECT if card_file.is_some() => {
            let (handle, f) = card_file.expect("checked");
            send_card_data(ctx.w, shared, tid, handle, 0, f.bytes, &line).await?;
            RC_OK
        }
        OP_GET_PARTIAL_OBJECT => match (card_file, params.get(1), params.get(2)) {
            (Some((handle, f)), Some(&offset), Some(&max)) if u64::from(offset) <= f.bytes => {
                let len = (f.bytes - u64::from(offset)).min(u64::from(max));
                line.push_str(&format!(" handle={handle:#x} offset={offset} max={max}"));
                send_card_data(ctx.w, shared, tid, handle, u64::from(offset), len, &line).await?;
                resp_params.push(len as u32);
                RC_OK
            }
            (None, ..) => RC_INVALID_OBJECT_HANDLE,
            _ => RC_INVALID_PARAMETER,
        },
        OP_GET_OBJECT_INFO | OP_GET_OBJECT if params.first() == Some(&HANDLE_CAPTURED_STILL) => {
            match captured_still(ctx, op, tid).await? {
                Ok(changed) => {
                    changed_prop = changed;
                    RC_OK
                }
                Err(rc) => rc,
            }
        }
        OP_GET_OBJECT_INFO | OP_GET_OBJECT => {
            let answer = match (op, params.first()) {
                (OP_GET_OBJECT_INFO, Some(&HANDLE_LIVE_VIEW)) => {
                    let seen = shared.live_view_infos.fetch_add(1, Ordering::SeqCst);
                    if seen < shared.config.live_view_info_denied {
                        Err(RC_ACCESS_DENIED)
                    } else {
                        Ok(frames::object_info())
                    }
                }
                (OP_GET_OBJECT, Some(&HANDLE_LIVE_VIEW)) => {
                    let busy_every = shared.config.live_view_busy_every;
                    shared.live_view.lock().unwrap().next(busy_every)
                }
                _ => Err(RC_INVALID_OBJECT_HANDLE),
            };
            match answer {
                Ok(dataset) => {
                    send_data_in(ctx.w, tid, &dataset).await?;
                    RC_OK
                }
                Err(rc) => rc,
            }
        }
        OP_SDIO_CONNECT => {
            if params.first() == Some(&(*handshake_phase as u32 + 1)) {
                *handshake_phase += 1;
                if shared.config.sdio_connect_data {
                    send_data_in(ctx.w, tid, &[0u8; 8]).await?;
                }
                RC_OK
            } else {
                RC_INVALID_PARAMETER
            }
        }
        OP_SDIO_GET_EXT_DEVICE_INFO => {
            let codes = shared.props.lock().unwrap().codes();
            send_data_in(ctx.w, tid, &encode_ext_device_info(&codes)).await?;
            RC_OK
        }
        // Other Sony operations are refused until the handshake completes.
        0x9200..=0x92FF if *handshake_phase < 3 => RC_ACCESS_DENIED,
        OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO
            if shared.config.props_param_hangs && !params.is_empty() =>
        {
            *ctx.hung = true;
            RC_OK
        }
        OP_SDIO_GET_EXT_DEVICE_PROP if shared.config.single_prop_read_hangs => {
            *ctx.hung = true;
            RC_OK
        }
        OP_SDIO_GET_ALL_EXT_DEVICE_PROP_INFO => {
            let blob = match params.first() {
                // No parameter is how libgphoto2 asks; every property.
                None => Some(shared.props.lock().unwrap().encode_all()),
                Some(0) => Some(shared.props.lock().unwrap().encode_all()),
                Some(1) => Some(shared.props.lock().unwrap().take_changed()),
                _ => None,
            };
            match blob {
                Some(blob) => {
                    send_data_in(ctx.w, tid, &blob).await?;
                    RC_OK
                }
                None => RC_INVALID_PARAMETER,
            }
        }
        OP_SDIO_GET_EXT_DEVICE_PROP => {
            let entry = prop_code(&params).and_then(|c| shared.props.lock().unwrap().encode_one(c));
            match entry {
                Some(entry) => {
                    send_data_in(ctx.w, tid, &entry).await?;
                    RC_OK
                }
                None => RC_INVALID_PARAMETER,
            }
        }
        OP_SDIO_SET_EXT_DEVICE_PROP_VALUE if shared.config.write_mode_step => RC_INVALID_PARAMETER,
        OP_SDIO_SET_EXT_DEVICE_PROP_VALUE => match (prop_code(&params), &data) {
            (Some(prop), Some(value)) => {
                let result = shared.props.lock().unwrap().write(prop, value);
                match result {
                    Ok(changed) => {
                        changed_prop = changed.then_some(prop);
                        RC_OK
                    }
                    Err(e) => e.response_code(),
                }
            }
            _ => RC_INVALID_PARAMETER,
        },
        OP_SDIO_CONTROL_DEVICE
            if shared.config.fail_nth_control
                == Some(shared.controls_seen.fetch_add(1, Ordering::SeqCst) + 1) =>
        {
            line.push_str(" refused");
            RC_DEVICE_BUSY
        }
        OP_SDIO_CONTROL_DEVICE => match (prop_code(&params), &data) {
            (Some(control), Some(value)) => match apply_control(shared, control, value) {
                Ok(changed) => {
                    changed_prop = changed;
                    RC_OK
                }
                Err(rc) => rc,
            },
            _ => RC_INVALID_PARAMETER,
        },
        _ => RC_OPERATION_NOT_SUPPORTED,
    };
    if *ctx.hung {
        line.push_str(" unanswered");
        shared.log.lock().unwrap().push(line);
        return Ok(());
    }
    shared.log.lock().unwrap().push(line);
    send(
        ctx.w,
        &Packet::CmdResponse {
            code,
            tid,
            params: resp_params,
        },
    )
    .await?;
    if let Some(prop) = changed_prop {
        send_prop_changed(ctx.event, prop).await?;
    }
    Ok(())
}

/// Sends bytes `offset..offset + len` of the card file `handle` as a data
/// phase, generated a packet at a time. With `drop_during_card_download_at`
/// set, the first time the card data sent reaches it the rest is held back,
/// `line` is logged with ` dropped`, and the session ends.
async fn send_card_data(
    w: &mut (impl AsyncWriteExt + Unpin),
    shared: &Shared,
    tid: u32,
    handle: u32,
    offset: u64,
    len: u64,
    line: &str,
) -> Result<(), Error> {
    send(
        w,
        &Packet::StartData {
            tid,
            total_len: len,
        },
    )
    .await?;
    let mut done = 0u64;
    loop {
        let size = (len - done).min(CARD_PACKET as u64) as usize;
        let sent_before = shared.card_sent.load(Ordering::SeqCst);
        if let Some(at) = shared.config.drop_during_card_download_at {
            if sent_before + size as u64 > at && !shared.card_dropped.swap(true, Ordering::SeqCst) {
                let head = (at.saturating_sub(sent_before) as usize).min(size);
                let payload = card::bytes(handle, offset + done, head);
                send(w, &Packet::Data { tid, payload }).await?;
                shared.card_sent.fetch_add(head as u64, Ordering::SeqCst);
                shared.log.lock().unwrap().push(format!("{line} dropped"));
                // Ending the session closes both sockets.
                return Err(Error::Disconnected);
            }
        }
        let payload = card::bytes(handle, offset + done, size);
        done += size as u64;
        shared.card_sent.fetch_add(size as u64, Ordering::SeqCst);
        if done == len {
            return send(w, &Packet::EndData { tid, payload }).await;
        }
        send(w, &Packet::Data { tid, payload }).await?;
    }
}

/// Answers GetObjectInfo or GetObject on the captured still handle. The
/// inner `Ok` holds the property GetObject changed (0xD215); the inner `Err`
/// the response code when no picture is waiting.
async fn captured_still<W: AsyncWriteExt + Unpin>(
    ctx: &mut Ctx<'_, W>,
    op: u16,
    tid: u32,
) -> Result<Result<Option<u16>, u16>, Error> {
    let shared = ctx.shared;
    let waiting = (shared.controls.lock().unwrap())
        .capture_jpeg(frames::capture_jpeg)
        .map(|(shot, jpeg)| (shot, jpeg.to_vec()));
    let Some((shot, jpeg)) = waiting else {
        return Ok(Err(RC_INVALID_OBJECT_HANDLE));
    };
    if op == OP_GET_OBJECT_INFO {
        send_data_in(ctx.w, tid, &frames::capture_object_info(shot, jpeg.len())).await?;
        return Ok(Ok(None));
    }
    if shared.config.drop_during_capture {
        let total_len = jpeg.len() as u64;
        send(ctx.w, &Packet::StartData { tid, total_len }).await?;
        let head = jpeg[..jpeg.len() / 2].to_vec();
        send(ctx.w, &Packet::Data { tid, payload: head }).await?;
        // Ending the session closes both sockets.
        return Err(Error::Disconnected);
    }
    match shared.config.capture_packet_pause {
        Some(pause) => send_data_in_paced(ctx.w, tid, &jpeg, pause).await?,
        None => send_data_in(ctx.w, tid, &jpeg).await?,
    }
    let mut controls = shared.controls.lock().unwrap();
    let mut props = shared.props.lock().unwrap();
    let popped = controls.pop_capture(&mut props);
    if let Some(after) = shared.config.restart_numbering_after {
        controls.restart_numbering_at(after);
    }
    Ok(Ok(match popped {
        Some((_, Outcome::Changed(prop))) => Some(prop),
        _ => None,
    }))
}

/// Runs one 0x9207 control and returns the property it changed, if any.
fn apply_control(shared: &Shared, code: u16, data: &[u8]) -> Result<Option<u16>, u16> {
    let mut controls = shared.controls.lock().unwrap();
    let mut props = shared.props.lock().unwrap();
    let recording = |props: &PropStore| {
        props
            .get(PROP_MOVIE_REC_STATE)
            .and_then(|p| p.current.as_u64())
            .is_some_and(|v| v != 0)
    };
    let was_recording = recording(&props);
    let outcome = controls.apply(&mut props, code, data)?;
    if was_recording && !recording(&props) {
        // The clip just recorded lands on the card.
        let mut card = shared.card.lock().unwrap();
        let clips = card.iter().filter(|f| f.format == FMT_MP4).count();
        card.push(card::new_clip(clips as u32 + 1));
    }
    match outcome {
        Outcome::Unchanged => Ok(None),
        Outcome::Changed(prop) => Ok(Some(prop)),
        Outcome::StartFocus => {
            // The session applies the change and emits the event when the
            // timer fires, as if the camera found focus by itself.
            let session = shared.current.lock().unwrap().clone();
            if let (Some(session), Some(value)) = (session, Controls::focus_found_value(&props)) {
                let timer = tokio::spawn(async move {
                    tokio::time::sleep(FOCUS_DELAY).await;
                    let _ = session
                        .send(SimCommand::SetProperty {
                            code: PROP_FOCUS_FOUND,
                            value,
                        })
                        .await;
                });
                controls.set_focus_timer(timer.abort_handle());
            }
            Ok(None)
        }
    }
}

/// The property code in a command's first parameter.
fn prop_code(params: &[u32]) -> Option<u16> {
    params.first().and_then(|&p| u16::try_from(p).ok())
}

fn encode_ext_device_info(properties: &[u16]) -> Vec<u8> {
    let mut out = EXT_VERSION.to_le_bytes().to_vec();
    // Two standard PTP arrays: a u32 count, then that many u16 codes.
    for list in [properties, &EXT_CONTROLS[..]] {
        out.extend((list.len() as u32).to_le_bytes());
        for code in list {
            out.extend(code.to_le_bytes());
        }
    }
    out
}

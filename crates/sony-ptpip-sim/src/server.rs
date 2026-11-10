use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sony_ptpip::error::Error;
use sony_ptpip::ptp::codes::*;
use sony_ptpip::ptp::{encode_device_info, DeviceInfo};
use sony_ptpip::ptpip::{read_frame, Packet};
use tokio::io::AsyncWriteExt;
use tokio::net::tcp::OwnedReadHalf;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinSet;

use crate::{SimCommand, SimConfig, SimHandle};

const PROTOCOL_VERSION: u32 = 0x0001_0000;
const SIM_GUID: [u8; 16] = *b"rimix-simulator!";
const DEVICE_VERSION: &str = "2.00";
const VENDOR_EXT_ID: u32 = 0x11;
/// How long a command connection waits for its event connection.
const EVENT_SOCKET_WAIT: Duration = Duration::from_secs(10);
const SUPPORTED_OPS: [u16; 11] = [
    OP_GET_DEVICE_INFO,
    OP_OPEN_SESSION,
    OP_CLOSE_SESSION,
    OP_GET_OBJECT_INFO,
    OP_GET_OBJECT,
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
}

/// Starts the simulator on 127.0.0.1 with an OS-chosen port.
pub async fn spawn(config: SimConfig) -> SimHandle {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind simulator listener");
    let addr = listener.local_addr().expect("listener address");
    let log: Log = Arc::default();
    let shared = Arc::new(Shared {
        config,
        log: log.clone(),
        next_conn: AtomicU32::new(1),
        pending: Mutex::default(),
        current: Mutex::default(),
    });
    let (tx, rx) = mpsc::channel(16);
    tokio::spawn(run(listener, shared, rx));
    SimHandle {
        addr,
        events: tx,
        log,
    }
}

async fn run(listener: TcpListener, shared: Arc<Shared>, mut control: mpsc::Receiver<SimCommand>) {
    // Dropping the set on exit aborts every connection and session task.
    let mut tasks = JoinSet::new();
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
    }
    .run(ctl_rx)
    .await
}

/// One paired command + event socket.
struct Session {
    shared: Arc<Shared>,
    stream: TcpStream,
    event: TcpStream,
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
            mut event,
        } = self;
        let (r, mut w) = stream.into_split();
        let (pkt_tx, mut packets) = mpsc::channel(16);
        let reader = tokio::spawn(read_packets(r, pkt_tx));
        let result = loop {
            tokio::select! {
                p = packets.recv() => {
                    let p = match p {
                        Some(Ok(p)) => p,
                        Some(Err(e)) => break Err(e),
                        None => break Ok(()),
                    };
                    if let Err(e) = handle_packet(p, &mut packets, &mut w, &shared).await {
                        break Err(e);
                    }
                }
                cmd = control.recv() => match cmd {
                    Some(SimCommand::SetProperty { code, .. }) => {
                        let ev = Packet::Event { code: EV_SONY_DEVICE_PROP_CHANGED, tid: 0, params: vec![code as u32] };
                        if let Err(e) = send(&mut event, &ev).await {
                            break Err(e);
                        }
                    }
                    Some(SimCommand::DropConnection) | Some(SimCommand::Shutdown) | None => break Ok(()),
                },
            }
        };
        // Dropping the read half (via abort), the write half and the event
        // stream closes both sockets.
        reader.abort();
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

async fn handle_packet(
    p: Packet,
    packets: &mut mpsc::Receiver<Result<Packet, Error>>,
    w: &mut (impl AsyncWriteExt + Unpin),
    shared: &Shared,
) -> Result<(), Error> {
    let Packet::CmdRequest {
        data_phase,
        op,
        tid,
        ..
    } = p
    else {
        tracing::debug!(?p, "simulator ignoring packet");
        return Ok(());
    };
    let mut line = format!("op {op:#06x} tid {tid}");
    if data_phase == 2 {
        let data = read_data_out(packets).await?;
        line.push_str(&format!(" data-out {}", data.len()));
    }
    shared.log.lock().unwrap().push(line);
    if let Some((slow_op, delay)) = shared.config.delay_op {
        if slow_op == op {
            tokio::time::sleep(delay).await;
        }
    }

    let code = match op {
        OP_OPEN_SESSION | OP_CLOSE_SESSION => RC_OK,
        OP_GET_DEVICE_INFO => {
            let info = DeviceInfo {
                vendor_ext_id: VENDOR_EXT_ID,
                model: shared.config.model.clone(),
                device_version: DEVICE_VERSION.into(),
                operations: SUPPORTED_OPS.to_vec(),
            };
            send_data_in(w, tid, &encode_device_info(&info)).await?;
            RC_OK
        }
        _ => RC_OPERATION_NOT_SUPPORTED,
    };
    send(
        w,
        &Packet::CmdResponse {
            code,
            tid,
            params: Vec::new(),
        },
    )
    .await
}

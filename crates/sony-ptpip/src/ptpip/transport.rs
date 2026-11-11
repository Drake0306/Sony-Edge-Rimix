//! PTP/IP connection: command socket with transactions plus an event socket.

use std::future::Future;
use std::io::ErrorKind;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::error::Error;
use crate::ptp::codes::{OP_CLOSE_SESSION, OP_OPEN_SESSION, RC_OK};
use crate::ptpip::packet::{read_frame, Packet};

/// PTP/IP protocol version we announce (1.0).
const PROTOCOL_VERSION: u32 = 0x0001_0000;
/// Session id used for OpenSession.
const SESSION_ID: u32 = 1;
/// `StartData.total_len` value meaning "length not known in advance".
const UNKNOWN_LEN: u64 = 0xFFFF_FFFF_FFFF_FFFF;
const EVENT_CHANNEL_CAPACITY: usize = 64;
/// CmdRequest data-phase field: no data or data from the camera.
const DATA_PHASE_NONE_OR_IN: u32 = 1;
/// CmdRequest data-phase field: data to the camera.
const DATA_PHASE_OUT: u32 = 2;

/// Settings for [`PtpIpConnection::connect`].
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    /// Identifies this host to the camera; keep it stable so pairing is remembered.
    pub guid: [u8; 16],
    /// Host name shown on the camera's pairing prompt.
    pub name: String,
    /// Limit for each TCP connect and each command round-trip.
    ///
    /// A command that hits this limit leaves the command stream part-way
    /// through a frame, so the connection becomes unusable (every later
    /// command returns [`Error::Disconnected`]) and must be reopened.
    pub timeout: Duration,
    /// How long to wait for InitCommandAck while the camera shows the pairing prompt.
    pub pairing_wait: Duration,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        ConnectOptions {
            guid: [0; 16],
            name: "Rimix".into(),
            timeout: Duration::from_secs(5),
            pairing_wait: Duration::from_secs(60),
        }
    }
}

/// An event received on the event socket.
#[derive(Debug, Clone)]
pub struct PtpEvent {
    pub code: u16,
    pub params: Vec<u32>,
}

/// Payload of a data-in phase.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataIn(pub Vec<u8>);

/// An open PTP/IP session with a camera.
pub struct PtpIpConnection {
    reader: OwnedReadHalf,
    writer: OwnedWriteHalf,
    next_tid: u32,
    /// Set while a transaction is in flight. If it is still set when the next
    /// one starts, the previous transaction timed out or its future was
    /// dropped, and the command stream may be mid-frame.
    poisoned: bool,
    timeout: Duration,
    events: broadcast::Sender<PtpEvent>,
    disconnected: Arc<AtomicBool>,
    event_task: JoinHandle<()>,
}

/// Maps "the peer went away" I/O errors to [`Error::Disconnected`].
fn map_io(e: Error) -> Error {
    match e {
        Error::Io(io)
            if matches!(
                io.kind(),
                ErrorKind::UnexpectedEof
                    | ErrorKind::ConnectionReset
                    | ErrorKind::ConnectionAborted
                    | ErrorKind::BrokenPipe
            ) =>
        {
            Error::Disconnected
        }
        other => other,
    }
}

async fn with_timeout<T>(
    limit: Duration,
    fut: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::time::timeout(limit, fut)
        .await
        .map_err(|_| Error::Timeout)?
}

/// Like [`with_timeout`] when `idle` is set; without it, nothing is limited.
async fn limited<T>(
    idle: Option<Duration>,
    fut: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    match idle {
        Some(limit) => with_timeout(limit, fut).await,
        None => fut.await,
    }
}

async fn tcp_connect(addr: SocketAddr, limit: Duration) -> Result<TcpStream, Error> {
    let s = with_timeout(limit, async { Ok(TcpStream::connect(addr).await?) }).await?;
    s.set_nodelay(true)?;
    Ok(s)
}

async fn send<W: AsyncWriteExt + Unpin>(w: &mut W, p: &Packet) -> Result<(), Error> {
    w.write_all(&p.encode()).await.map_err(|e| map_io(e.into()))
}

async fn recv<R: tokio::io::AsyncRead + Unpin>(r: &mut R) -> Result<Packet, Error> {
    let frame = read_frame(r).await.map_err(map_io)?;
    Packet::decode(&frame)
}

fn check_tid(got: u32, want: u32) -> Result<(), Error> {
    if got != want {
        return Err(Error::Protocol(format!(
            "transaction id {got} does not match {want}"
        )));
    }
    Ok(())
}

fn response(op: u16, tid: u32, p: Packet) -> Result<Vec<u32>, Error> {
    match p {
        Packet::CmdResponse {
            code,
            tid: rtid,
            params,
        } => {
            check_tid(rtid, tid)?;
            if code != RC_OK {
                return Err(Error::Response { code, op });
            }
            Ok(params)
        }
        other => Err(Error::Protocol(format!(
            "expected CmdResponse, got {other:?}"
        ))),
    }
}

/// Reads the event socket until it closes: forwards events to `tx` and
/// answers ProbeRequest with ProbeResponse.
async fn read_events(
    mut r: TcpStream,
    tx: broadcast::Sender<PtpEvent>,
    disconnected: Arc<AtomicBool>,
) {
    loop {
        match recv(&mut r).await {
            Ok(Packet::Event { code, params, .. }) => {
                // No receivers is fine: nobody is listening right now.
                let _ = tx.send(PtpEvent { code, params });
            }
            // The camera checks that the host is still there; an
            // unanswered probe may make it drop the link.
            Ok(Packet::ProbeRequest) => {
                if let Err(e) = send(&mut r, &Packet::ProbeResponse).await {
                    tracing::debug!(error = %e, "answering a probe failed");
                    disconnected.store(true, Ordering::SeqCst);
                    return;
                }
            }
            Ok(other) => tracing::debug!(?other, "ignoring packet on event socket"),
            Err(e) => {
                tracing::debug!(error = %e, "event socket closed");
                disconnected.store(true, Ordering::SeqCst);
                return;
            }
        }
    }
}

impl PtpIpConnection {
    /// Opens the command and event sockets, completes the PTP/IP handshake
    /// and opens PTP session 1. Returns the camera's InitCommandAck.
    pub async fn connect(
        addr: SocketAddr,
        opts: ConnectOptions,
    ) -> Result<(PtpIpConnection, Packet), Error> {
        let cmd = tcp_connect(addr, opts.timeout).await?;
        let (mut reader, mut writer) = cmd.into_split();
        send(
            &mut writer,
            &Packet::InitCommandRequest {
                guid: opts.guid,
                name: opts.name.clone(),
                version: PROTOCOL_VERSION,
            },
        )
        .await?;
        // Only this wait means "nobody pressed OK"; a connect timeout is not pairing.
        let ack = tokio::time::timeout(opts.pairing_wait, recv(&mut reader))
            .await
            .map_err(|_| Error::PairingTimeout)??;
        let conn_number = match &ack {
            Packet::InitCommandAck { conn_number, .. } => *conn_number,
            Packet::InitFail { reason } => {
                return Err(Error::Protocol(format!(
                    "camera refused the connection (reason {reason:#x})"
                )))
            }
            other => {
                return Err(Error::Protocol(format!(
                    "expected InitCommandAck, got {other:?}"
                )))
            }
        };

        let mut ev = tcp_connect(addr, opts.timeout).await?;
        send(&mut ev, &Packet::InitEventRequest { conn_number }).await?;
        match with_timeout(opts.timeout, recv(&mut ev)).await? {
            Packet::InitEventAck => {}
            other => {
                return Err(Error::Protocol(format!(
                    "expected InitEventAck, got {other:?}"
                )))
            }
        }

        let (events, _) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
        let disconnected = Arc::new(AtomicBool::new(false));
        let event_task = tokio::spawn(read_events(ev, events.clone(), disconnected.clone()));
        let mut conn = PtpIpConnection {
            reader,
            writer,
            next_tid: 1,
            poisoned: false,
            timeout: opts.timeout,
            events,
            disconnected,
            event_task,
        };
        conn.command(OP_OPEN_SESSION, &[SESSION_ID]).await?;
        Ok((conn, ack))
    }

    fn begin(&mut self) -> Result<u32, Error> {
        if self.is_disconnected() {
            return Err(Error::Disconnected);
        }
        self.poisoned = true;
        let tid = self.next_tid;
        self.next_tid = self.next_tid.wrapping_add(1).max(1);
        Ok(tid)
    }

    /// Clears the in-flight marker when the transaction ran to its response,
    /// leaving the stream at a frame boundary.
    fn finish<T>(&mut self, result: Result<T, Error>) -> Result<T, Error> {
        if matches!(result, Ok(_) | Err(Error::Response { .. })) {
            self.poisoned = false;
        }
        result
    }

    /// Runs a transaction with no data phase and returns the response parameters.
    ///
    /// A timeout, or dropping the returned future before it completes, makes
    /// the connection unusable: later commands return [`Error::Disconnected`]
    /// and the connection must be reopened.
    pub async fn command(&mut self, op: u16, params: &[u32]) -> Result<Vec<u32>, Error> {
        let tid = self.begin()?;
        let limit = self.timeout;
        let result = with_timeout(limit, async {
            self.send_request(DATA_PHASE_NONE_OR_IN, op, tid, params)
                .await?;
            let p = match recv(&mut self.reader).await? {
                // Some cameras send data for operations that are documented
                // without it; read it so the stream stays at a frame boundary.
                Packet::StartData {
                    tid: rtid,
                    total_len,
                } if rtid == tid => {
                    let data = self
                        .read_data_phase(tid, total_len, None, &mut |_| {})
                        .await?;
                    tracing::warn!(
                        "operation {op:#06x} sent an unexpected data phase ({} bytes); accepted",
                        data.len()
                    );
                    recv(&mut self.reader).await?
                }
                p => p,
            };
            response(op, tid, p)
        })
        .await;
        self.finish(result)
    }

    /// Runs a transaction with a data phase from the camera; returns the data
    /// and the response parameters.
    ///
    /// A timeout, or dropping the returned future before it completes, makes
    /// the connection unusable: later commands return [`Error::Disconnected`]
    /// and the connection must be reopened.
    pub async fn command_data_in(
        &mut self,
        op: u16,
        params: &[u32],
    ) -> Result<(Vec<u8>, Vec<u32>), Error> {
        let tid = self.begin()?;
        let limit = self.timeout;
        let result = with_timeout(
            limit,
            self.data_in_phase(op, params, tid, None, &mut |_| {}),
        )
        .await;
        self.finish(result)
    }

    /// Like [`command_data_in`](Self::command_data_in), but for large objects
    /// such as stills: there is no limit on the whole transaction, only on
    /// each wait for the next packet. `idle` without a packet from the
    /// camera ends it with [`Error::Timeout`] (and makes the connection
    /// unusable, as any timeout does).
    pub async fn command_data_in_idle(
        &mut self,
        op: u16,
        params: &[u32],
        idle: Duration,
    ) -> Result<(Vec<u8>, Vec<u32>), Error> {
        self.command_data_in_streaming(op, params, idle, &mut |_| {})
            .await
    }

    /// Like [`command_data_in_idle`](Self::command_data_in_idle), and calls
    /// `on_data(received)` with the bytes received so far after every Data
    /// and EndData packet.
    pub async fn command_data_in_streaming(
        &mut self,
        op: u16,
        params: &[u32],
        idle: Duration,
        on_data: &mut (dyn FnMut(u64) + Send),
    ) -> Result<(Vec<u8>, Vec<u32>), Error> {
        let tid = self.begin()?;
        let result = self
            .data_in_phase(op, params, tid, Some(idle), on_data)
            .await;
        self.finish(result)
    }

    /// Sends the request and reads the data phase and the response. With
    /// `idle`, each packet wait is limited to it; without, nothing is (the
    /// caller limits the whole transaction).
    async fn data_in_phase(
        &mut self,
        op: u16,
        params: &[u32],
        tid: u32,
        idle: Option<Duration>,
        on_data: &mut (dyn FnMut(u64) + Send),
    ) -> Result<(Vec<u8>, Vec<u32>), Error> {
        limited(
            idle,
            self.send_request(DATA_PHASE_NONE_OR_IN, op, tid, params),
        )
        .await?;
        let total_len = match limited(idle, recv(&mut self.reader)).await? {
            Packet::StartData {
                tid: rtid,
                total_len,
            } => {
                check_tid(rtid, tid)?;
                total_len
            }
            // The camera may answer with no data phase at all (usually an error).
            p @ Packet::CmdResponse { .. } => return Ok((Vec::new(), response(op, tid, p)?)),
            other => {
                return Err(Error::Protocol(format!(
                    "expected StartData, got {other:?}"
                )))
            }
        };
        let data = self.read_data_phase(tid, total_len, idle, on_data).await?;
        let p = limited(idle, recv(&mut self.reader)).await?;
        Ok((data, response(op, tid, p)?))
    }

    /// Reads the Data and EndData packets that follow a StartData announcing
    /// `total_len` bytes, and checks the length. `idle` limits each packet
    /// wait as in [`data_in_phase`](Self::data_in_phase).
    async fn read_data_phase(
        &mut self,
        tid: u32,
        total_len: u64,
        idle: Option<Duration>,
        on_data: &mut (dyn FnMut(u64) + Send),
    ) -> Result<Vec<u8>, Error> {
        let mut data = Vec::new();
        loop {
            match limited(idle, recv(&mut self.reader)).await? {
                Packet::Data { tid: rtid, payload } => {
                    check_tid(rtid, tid)?;
                    data.extend_from_slice(&payload);
                    on_data(data.len() as u64);
                }
                Packet::EndData { tid: rtid, payload } => {
                    check_tid(rtid, tid)?;
                    data.extend_from_slice(&payload);
                    on_data(data.len() as u64);
                    break;
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "expected Data or EndData, got {other:?}"
                    )))
                }
            }
        }
        if total_len != UNKNOWN_LEN && data.len() as u64 != total_len {
            return Err(Error::Protocol(format!(
                "data phase carried {} bytes, announced {total_len}",
                data.len()
            )));
        }
        Ok(data)
    }

    /// Runs a transaction with a data phase to the camera; returns the
    /// response parameters.
    ///
    /// A timeout, or dropping the returned future before it completes, makes
    /// the connection unusable: later commands return [`Error::Disconnected`]
    /// and the connection must be reopened.
    pub async fn command_data_out(
        &mut self,
        op: u16,
        params: &[u32],
        data: &[u8],
    ) -> Result<Vec<u32>, Error> {
        let tid = self.begin()?;
        let limit = self.timeout;
        let result = with_timeout(limit, async {
            self.send_request(DATA_PHASE_OUT, op, tid, params).await?;
            send(
                &mut self.writer,
                &Packet::StartData {
                    tid,
                    total_len: data.len() as u64,
                },
            )
            .await?;
            send(
                &mut self.writer,
                &Packet::EndData {
                    tid,
                    payload: data.to_vec(),
                },
            )
            .await?;
            let p = recv(&mut self.reader).await?;
            response(op, tid, p)
        })
        .await;
        self.finish(result)
    }

    async fn send_request(
        &mut self,
        data_phase: u32,
        op: u16,
        tid: u32,
        params: &[u32],
    ) -> Result<(), Error> {
        send(
            &mut self.writer,
            &Packet::CmdRequest {
                data_phase,
                op,
                tid,
                params: params.to_vec(),
            },
        )
        .await
    }

    /// The limit each command round-trip runs under ([`ConnectOptions::timeout`]).
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Subscribes to camera events. Events sent before subscribing are not seen.
    pub fn events(&self) -> broadcast::Receiver<PtpEvent> {
        self.events.subscribe()
    }

    /// True once the event socket has closed or a transaction was cut short
    /// (timeout or dropped future); every later command fails with
    /// [`Error::Disconnected`] and the connection must be reopened.
    pub fn is_disconnected(&self) -> bool {
        self.poisoned || self.disconnected.load(Ordering::SeqCst)
    }

    /// Closes the PTP session and drops both sockets.
    pub async fn close(mut self) -> Result<(), Error> {
        self.command(OP_CLOSE_SESSION, &[]).await?;
        Ok(())
    }
}

impl std::fmt::Debug for PtpIpConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtpIpConnection")
            .field("next_tid", &self.next_tid)
            .field("disconnected", &self.is_disconnected())
            .finish_non_exhaustive()
    }
}

impl Drop for PtpIpConnection {
    fn drop(&mut self) {
        self.event_task.abort();
    }
}

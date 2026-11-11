//! Webcam mode: the camera's live view served on this computer as a video
//! feed (motion JPEG over HTTP), for a program that turns a feed into a
//! system camera, such as OBS and its virtual camera.
//!
//! macOS only lets an app add a camera to the system from a camera
//! extension signed with an Apple Developer ID, which Rimix does not have.
//! So Rimix serves the picture and another program presents it:
//!
//! - `http://127.0.0.1:8717/` is a page that fills its window with the
//!   feed (an OBS browser source, or a browser tab);
//! - `/stream.mjpg` is the feed itself (an OBS media source, ffmpeg, VLC);
//! - `/frame.jpg` is the latest frame alone.
//!
//! Only this computer can connect: the server listens on the loopback
//! address. Sound is not part of it; the computer's own microphone is used
//! as usual.

use std::io::Cursor;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::{Duration, Instant};

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use sony_ptpip::liveview::LiveViewFrame;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// The port the feed is served on, so the address can be typed into
/// another program once and keeps working. If it is taken, any free port.
pub const PORT: u16 = 8717;
/// Between the parts of the stream.
const BOUNDARY: &str = "rimixframe";
/// How often a connection looks for a new frame.
const POLL: Duration = Duration::from_millis(5);
/// With no new frame for this long the last one is sent again, so a viewer
/// that left is noticed and one that stayed does not give up.
const KEEP_ALIVE: Duration = Duration::from_secs(1);
/// Longest request head read.
const MAX_REQUEST: usize = 4096;
/// JPEG quality of a frame Rimix made smaller.
const JPEG_QUALITY: u8 = 85;

/// The latest live view frame of the camera being used as a webcam.
pub type FrameSource = Arc<dyn Fn() -> Option<Arc<LiveViewFrame>> + Send + Sync>;

/// How much picture the feed carries.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    /// Every frame as the camera sends it.
    #[default]
    Full,
    /// Three quarters of the size, at most 20 frames a second.
    Medium,
    /// Half the size, at most 10 frames a second.
    Low,
}

impl Quality {
    /// The size of the feed's picture for a camera frame of `width` by
    /// `height`, kept even as video encoders want.
    pub fn size(self, width: u32, height: u32) -> (u32, u32) {
        let (num, den) = match self {
            Quality::Full => return (width, height),
            Quality::Medium => (3, 4),
            Quality::Low => (1, 2),
        };
        let even = |n: u32| ((n * num / den) & !1).max(2);
        (even(width), even(height))
    }

    /// The shortest time between two frames of the feed.
    fn frame_interval(self) -> Duration {
        match self {
            Quality::Full => Duration::ZERO,
            Quality::Medium => Duration::from_millis(50),
            Quality::Low => Duration::from_millis(100),
        }
    }
}

/// What the webcam panel shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WebcamStatus {
    pub enabled: bool,
    /// The page that shows the feed; `None` while off.
    pub url: Option<String>,
    /// The feed itself, for programs that take a video address.
    pub stream_url: Option<String>,
    pub quality: Quality,
    /// The size of the camera's own frames, once one has been seen.
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Programs reading the feed right now.
    pub viewers: usize,
}

#[derive(Default)]
struct Shared {
    quality: StdMutex<Quality>,
    viewers: AtomicUsize,
    /// The size of the camera's frames.
    size: StdMutex<Option<(u32, u32)>>,
}

struct Running {
    addr: SocketAddr,
    task: JoinHandle<()>,
}

/// The feed server; off until [`WebcamService::start`].
#[derive(Default)]
pub struct WebcamService {
    running: StdMutex<Option<Running>>,
    shared: Arc<Shared>,
}

fn lock<T>(m: &StdMutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl WebcamService {
    /// Starts serving frames from `source` on [`PORT`], or on any free port
    /// when that one is taken. Already running, it keeps its address.
    pub async fn start(&self, source: FrameSource) -> Result<SocketAddr, String> {
        match self.start_on(PORT, source.clone()).await {
            Ok(addr) => Ok(addr),
            Err(e) => {
                tracing::warn!(port = PORT, error = %e, "the webcam feed's usual port is taken; using another");
                self.start_on(0, source).await
            }
        }
    }

    /// [`WebcamService::start`] on a given port; 0 lets the system choose.
    pub async fn start_on(&self, port: u16, source: FrameSource) -> Result<SocketAddr, String> {
        if let Some(running) = lock(&self.running).as_ref() {
            return Ok(running.addr);
        }
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port))
            .await
            .map_err(|e| format!("Cannot serve the webcam feed on port {port}: {e}"))?;
        let addr = listener.local_addr().map_err(|e| e.to_string())?;
        let shared = self.shared.clone();
        let task = tokio::spawn(async move {
            let mut viewers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => match accepted {
                        Ok((socket, _)) => {
                            viewers.spawn(serve(socket, source.clone(), shared.clone()));
                        }
                        Err(e) => tracing::debug!(error = %e, "webcam feed: accept failed"),
                    },
                    // Finished connections are reaped as they end.
                    Some(_) = viewers.join_next() => {}
                }
            }
        });
        let mut running = lock(&self.running);
        if let Some(first) = running.as_ref() {
            // Another start won the race while this one was binding.
            task.abort();
            return Ok(first.addr);
        }
        tracing::info!(%addr, "webcam feed started");
        *running = Some(Running { addr, task });
        Ok(addr)
    }

    /// Stops the feed and drops every viewer.
    pub fn stop(&self) {
        if let Some(running) = lock(&self.running).take() {
            // Aborting the accept task drops its viewers with it.
            running.task.abort();
            self.shared.viewers.store(0, Ordering::SeqCst);
            tracing::info!("webcam feed stopped");
        }
    }

    pub fn set_quality(&self, quality: Quality) {
        *lock(&self.shared.quality) = quality;
    }

    pub fn status(&self) -> WebcamStatus {
        let addr = lock(&self.running).as_ref().map(|r| r.addr);
        let size = *lock(&self.shared.size);
        WebcamStatus {
            enabled: addr.is_some(),
            url: addr.map(|a| format!("http://{a}/")),
            stream_url: addr.map(|a| format!("http://{a}/stream.mjpg")),
            quality: *lock(&self.shared.quality),
            width: size.map(|s| s.0),
            height: size.map(|s| s.1),
            viewers: self.shared.viewers.load(Ordering::SeqCst),
        }
    }
}

/// Counts a viewer of the stream for as long as it lives.
struct Viewer(Arc<Shared>);

impl Viewer {
    fn new(shared: &Arc<Shared>) -> Viewer {
        shared.viewers.fetch_add(1, Ordering::SeqCst);
        Viewer(shared.clone())
    }
}

impl Drop for Viewer {
    fn drop(&mut self) {
        // Never below zero: `stop` may have reset the count already.
        let viewers = &self.0.viewers;
        let mut n = viewers.load(Ordering::SeqCst);
        while n > 0 {
            match viewers.compare_exchange_weak(n, n - 1, Ordering::SeqCst, Ordering::SeqCst) {
                Ok(_) => break,
                Err(now) => n = now,
            }
        }
    }
}

/// The path of a `GET` request head, without its query.
fn request_path(head: &str) -> Option<&str> {
    let mut words = head.lines().next()?.split_whitespace();
    if words.next()? != "GET" {
        return None;
    }
    let target = words.next()?;
    Some(target.split('?').next().unwrap_or(target))
}

/// Answers one connection: the page, one frame, or the stream until the
/// viewer leaves.
async fn serve(mut socket: TcpStream, source: FrameSource, shared: Arc<Shared>) {
    let mut head = Vec::new();
    let mut buf = [0u8; 1024];
    while !head.windows(4).any(|w| w == b"\r\n\r\n") && head.len() < MAX_REQUEST {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => head.extend_from_slice(&buf[..n]),
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let result = match request_path(&head) {
        Some("/" | "/index.html") => {
            respond(
                &mut socket,
                "200 OK",
                "text/html; charset=utf-8",
                PAGE.as_bytes(),
            )
            .await
        }
        Some("/frame.jpg") => match current(&source, &shared).await {
            Some(jpeg) => respond(&mut socket, "200 OK", "image/jpeg", &jpeg).await,
            None => {
                respond(
                    &mut socket,
                    "503 Service Unavailable",
                    "text/plain",
                    b"no frame yet",
                )
                .await
            }
        },
        Some("/stream.mjpg") => stream(&mut socket, source, &shared).await,
        _ => respond(&mut socket, "404 Not Found", "text/plain", b"not found").await,
    };
    if let Err(e) = result {
        tracing::debug!(error = %e, "webcam feed: viewer left");
    }
}

async fn respond(
    socket: &mut TcpStream,
    status: &str,
    kind: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(body).await?;
    socket.shutdown().await
}

/// The latest frame at the chosen quality, if the camera has sent one.
async fn current(source: &FrameSource, shared: &Arc<Shared>) -> Option<Vec<u8>> {
    let frame = source()?;
    let quality = *lock(&shared.quality);
    prepare(frame, quality, shared.clone()).await
}

/// `frame` as the feed sends it at `quality`: untouched for
/// [`Quality::Full`], made smaller otherwise (off the async threads).
/// `None` for a frame that is not a JPEG picture.
async fn prepare(
    frame: Arc<LiveViewFrame>,
    quality: Quality,
    shared: Arc<Shared>,
) -> Option<Vec<u8>> {
    tokio::task::spawn_blocking(move || {
        let (width, height) =
            image::ImageReader::with_format(Cursor::new(&frame.jpeg), ImageFormat::Jpeg)
                .into_dimensions()
                .ok()?;
        *lock(&shared.size) = Some((width, height));
        let (w, h) = quality.size(width, height);
        if (w, h) == (width, height) {
            return Some(frame.jpeg.clone());
        }
        let picture = image::load_from_memory_with_format(&frame.jpeg, ImageFormat::Jpeg).ok()?;
        let small = picture.resize_exact(w, h, FilterType::Triangle).into_rgb8();
        let mut out = Vec::with_capacity(frame.jpeg.len() / 2);
        JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
            .encode_image(&small)
            .ok()?;
        Some(out)
    })
    .await
    .ok()
    .flatten()
}

/// Sends frames as they come until the viewer leaves.
async fn stream(
    socket: &mut TcpStream,
    source: FrameSource,
    shared: &Arc<Shared>,
) -> std::io::Result<()> {
    let _viewer = Viewer::new(shared);
    socket
        .write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: multipart/x-mixed-replace; boundary={BOUNDARY}\r\nCache-Control: no-store\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .await?;
    let mut last: Option<Arc<LiveViewFrame>> = None;
    let mut last_part: Option<Vec<u8>> = None;
    let mut sent_at: Option<Instant> = None;
    loop {
        let quality = *lock(&shared.quality);
        let due = sent_at.is_none_or(|t| t.elapsed() >= quality.frame_interval());
        let fresh = source().filter(|f| last.as_ref().is_none_or(|l| !Arc::ptr_eq(l, f)));
        if let (true, Some(frame)) = (due, fresh) {
            last = Some(frame.clone());
            if let Some(jpeg) = prepare(frame, quality, shared.clone()).await {
                write_part(socket, &jpeg).await?;
                last_part = Some(jpeg);
                sent_at = Some(Instant::now());
            }
        } else if let (Some(part), Some(t)) = (&last_part, sent_at) {
            if t.elapsed() >= KEEP_ALIVE {
                write_part(socket, part).await?;
                sent_at = Some(Instant::now());
            }
        }
        tokio::time::sleep(POLL).await;
    }
}

async fn write_part(socket: &mut TcpStream, jpeg: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "--{BOUNDARY}\r\nContent-Type: image/jpeg\r\nContent-Length: {}\r\n\r\n",
        jpeg.len()
    );
    socket.write_all(head.as_bytes()).await?;
    socket.write_all(jpeg).await?;
    socket.write_all(b"\r\n").await?;
    socket.flush().await
}

/// The page that fills its window with the feed.
const PAGE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Rimix camera feed</title>
<style>
  html, body { margin: 0; height: 100%; background: #000; overflow: hidden; }
  img { display: block; width: 100vw; height: 100vh; object-fit: contain; }
</style>
</head>
<body><img src="/stream.mjpg" alt="Live picture from the camera"></body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    /// A real JPEG of `width` by `height`.
    fn jpeg(width: u32, height: u32, shade: u8) -> Vec<u8> {
        let picture = image::RgbImage::from_pixel(width, height, image::Rgb([shade, 90, 120]));
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 90)
            .encode_image(&picture)
            .unwrap();
        out
    }

    fn frame(jpeg: Vec<u8>) -> Arc<LiveViewFrame> {
        Arc::new(LiveViewFrame {
            jpeg,
            focus_frames: Vec::new(),
            face_frames: Vec::new(),
            tracking_frames: Vec::new(),
            level_deg: None,
            raw_meta: Vec::new(),
        })
    }

    /// A source that always gives `frame`.
    fn still(frame: Arc<LiveViewFrame>) -> FrameSource {
        Arc::new(move || Some(frame.clone()))
    }

    async fn get(addr: SocketAddr, path: &str) -> TcpStream {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket
            .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
            .await
            .unwrap();
        socket
    }

    /// Reads until `socket` has sent `needle` and then `after` more bytes.
    async fn read_past(
        socket: &mut TcpStream,
        got: &mut Vec<u8>,
        needle: &[u8],
        after: usize,
    ) -> usize {
        let find = |got: &[u8]| got.windows(needle.len()).position(|w| w == needle);
        tokio::time::timeout(Duration::from_secs(5), async {
            let mut buf = [0u8; 8192];
            loop {
                if let Some(at) = find(got) {
                    if got.len() >= at + needle.len() + after {
                        return at + needle.len();
                    }
                }
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0, "the server closed the connection early");
                got.extend_from_slice(&buf[..n]);
            }
        })
        .await
        .expect("the server sent what was expected in time")
    }

    /// The JPEG of the first part of the stream at `path`.
    async fn first_part(addr: SocketAddr) -> (String, Vec<u8>) {
        let mut socket = get(addr, "/stream.mjpg").await;
        let mut got = Vec::new();
        let head_end = read_past(&mut socket, &mut got, b"\r\n\r\n", 0).await;
        let head = String::from_utf8_lossy(&got[..head_end]).into_owned();
        let len_at = read_past(&mut socket, &mut got, b"Content-Length: ", 12).await;
        let digits: String = got[len_at..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .map(|&b| b as char)
            .collect();
        let len: usize = digits.parse().unwrap();
        let part_head = &got[len_at..];
        let body_offset = len_at + part_head.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        while got.len() < body_offset + len {
            let mut buf = [0u8; 8192];
            let n = socket.read(&mut buf).await.unwrap();
            assert!(n > 0);
            got.extend_from_slice(&buf[..n]);
        }
        (head, got[body_offset..body_offset + len].to_vec())
    }

    fn size_of(jpeg: &[u8]) -> (u32, u32) {
        image::ImageReader::with_format(Cursor::new(jpeg), ImageFormat::Jpeg)
            .into_dimensions()
            .unwrap()
    }

    #[tokio::test]
    async fn the_stream_is_motion_jpeg_carrying_the_cameras_frames_untouched() {
        let picture = jpeg(64, 36, 200);
        let service = WebcamService::default();
        let addr = service
            .start_on(0, still(frame(picture.clone())))
            .await
            .unwrap();
        assert!(addr.ip().is_loopback(), "only this computer may connect");
        let (head, part) = first_part(addr).await;
        assert!(head.starts_with("HTTP/1.1 200 OK"));
        assert!(head.contains("multipart/x-mixed-replace; boundary=rimixframe"));
        assert_eq!(part, picture);
        let status = service.status();
        assert_eq!((status.width, status.height), (Some(64), Some(36)));
        assert_eq!(
            status.url.as_deref(),
            Some(format!("http://{addr}/").as_str())
        );
        service.stop();
    }

    #[tokio::test]
    async fn lower_qualities_send_a_smaller_picture() {
        let service = WebcamService::default();
        let addr = service
            .start_on(0, still(frame(jpeg(640, 360, 80))))
            .await
            .unwrap();
        service.set_quality(Quality::Medium);
        assert_eq!(size_of(&first_part(addr).await.1), (480, 270));
        service.set_quality(Quality::Low);
        assert_eq!(size_of(&first_part(addr).await.1), (320, 180));
        service.set_quality(Quality::Full);
        assert_eq!(size_of(&first_part(addr).await.1), (640, 360));
        service.stop();
    }

    #[test]
    fn sizes_stay_even_and_full_is_the_cameras_own() {
        assert_eq!(Quality::Full.size(1024, 576), (1024, 576));
        assert_eq!(Quality::Medium.size(1024, 576), (768, 432));
        assert_eq!(Quality::Low.size(1024, 576), (512, 288));
        // 3:2 frames, whose scaled height would be odd.
        assert_eq!(Quality::Medium.size(1024, 682), (768, 510));
    }

    #[tokio::test]
    async fn a_new_frame_follows_the_first() {
        let frames = Arc::new(StdMutex::new(frame(jpeg(64, 36, 10))));
        let source: FrameSource = {
            let frames = frames.clone();
            Arc::new(move || Some(lock(&frames).clone()))
        };
        let service = WebcamService::default();
        let addr = service.start_on(0, source).await.unwrap();
        let mut socket = get(addr, "/stream.mjpg").await;
        let mut got = Vec::new();
        read_past(&mut socket, &mut got, b"--rimixframe", 0).await;
        let first = lock(&frames).jpeg.clone();
        let second = jpeg(64, 36, 240);
        assert_ne!(first, second);
        *lock(&frames) = frame(second.clone());
        // The whole second picture arrives, in a part of its own.
        read_past(&mut socket, &mut got, &second, 0).await;
        assert_eq!(got.windows(12).filter(|w| w == b"--rimixframe").count(), 2);
        assert_eq!(service.status().viewers, 1);
        drop(socket);
        service.stop();
    }

    #[tokio::test]
    async fn the_page_shows_the_stream_and_other_paths_are_not_found() {
        let service = WebcamService::default();
        let addr = service
            .start_on(0, still(frame(jpeg(64, 36, 1))))
            .await
            .unwrap();
        let read_all = |path: &'static str| async move {
            let mut socket = get(addr, path).await;
            let mut out = Vec::new();
            socket.read_to_end(&mut out).await.unwrap();
            String::from_utf8_lossy(&out).into_owned()
        };
        let page = read_all("/").await;
        assert!(page.starts_with("HTTP/1.1 200 OK"));
        assert!(page.contains("<img src=\"/stream.mjpg\""));
        assert!(read_all("/frame.jpg?x=1")
            .await
            .contains("Content-Type: image/jpeg"));
        assert!(read_all("/secret").await.starts_with("HTTP/1.1 404"));
        service.stop();
    }

    #[tokio::test]
    async fn starting_twice_keeps_the_address_and_stopping_ends_the_feed() {
        let service = WebcamService::default();
        let source = still(frame(jpeg(64, 36, 1)));
        let addr = service.start_on(0, source.clone()).await.unwrap();
        assert_eq!(service.start_on(0, source).await.unwrap(), addr);
        assert!(service.status().enabled);
        service.stop();
        let status = service.status();
        assert!(!status.enabled);
        assert_eq!(status.url, None);
        // Nothing listens there any more.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(TcpStream::connect(addr).await.is_err());
    }

    #[tokio::test]
    async fn a_camera_with_no_frame_yet_says_so_for_a_single_frame() {
        let service = WebcamService::default();
        let none: FrameSource = Arc::new(|| None);
        let addr = service.start_on(0, none).await.unwrap();
        let mut socket = get(addr, "/frame.jpg").await;
        let mut out = Vec::new();
        socket.read_to_end(&mut out).await.unwrap();
        assert!(String::from_utf8_lossy(&out).starts_with("HTTP/1.1 503"));
        service.stop();
    }
}

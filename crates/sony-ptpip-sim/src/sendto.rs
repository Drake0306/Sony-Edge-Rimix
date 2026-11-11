//! A pretend camera in "Send to Smartphone": the UPnP media server Sony's
//! bodies run on port 64321, as the clients that talk to real ones describe
//! it (see `sony_ptpip::sendto`). One folder holds a JPEG the camera sends
//! as it is, a RAW it only makes previews of, and a movie.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

/// How the pretend camera behaves.
#[derive(Debug, Clone)]
pub struct SendToSim {
    /// The folder everything hangs under: `PushRoot` (pictures chosen on
    /// the camera) or `PhotoRoot` (everything). The other is refused.
    pub root: &'static str,
    /// Most entries one Browse answers with, to make a client page.
    pub page: usize,
}

impl Default for SendToSim {
    fn default() -> Self {
        SendToSim {
            root: "PushRoot",
            page: 9999,
        }
    }
}

/// A running pretend camera.
pub struct SendToHandle {
    pub addr: SocketAddr,
    /// One line per request: `GET /path`, `X_TransferStart`, `Browse <id> <start>`.
    pub log: Arc<Mutex<Vec<String>>>,
    task: JoinHandle<()>,
}

impl Drop for SendToHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// The JPEG as it was taken: 300 KB.
pub fn original_jpeg() -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xD8];
    bytes.extend((0..300_000u32).map(|i| (i % 251) as u8));
    bytes.extend([0xFF, 0xD9]);
    bytes
}

fn small_jpeg(tag: u8) -> Vec<u8> {
    vec![0xFF, 0xD8, 0xFF, 0xE0, tag, 0xFF, 0xD9]
}

fn movie() -> Vec<u8> {
    (0..1_000_000u32).map(|i| (i % 239) as u8).collect()
}

fn item(addr: SocketAddr, id: u32, name: &str, sized: Option<usize>) -> String {
    let link = |kind: &str| format!("http://{addr}/{kind}_{name}");
    let file = sized
        .map(|size| {
            format!(
                "<res protocolInfo=\"http-get:*:application/octet-stream:*\" size=\"{size}\" resolution=\"6000x4000\">{}</res>",
                link("ORG")
            )
        })
        .unwrap_or_default();
    format!(
        "<item id=\"03_01_{id:010}\" parentID=\"x\" restricted=\"1\"><dc:title>{name}</dc:title>\
         <dc:date>2026-11-11T15:28:1{id}</dc:date><upnp:class>object.item.imageItem.photo</upnp:class>\
         <res protocolInfo=\"http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_TN;DLNA.ORG_CI=1\">{}</res>\
         <res protocolInfo=\"http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_LRG;DLNA.ORG_CI=1\" resolution=\"1616x1080\">{}</res>\
         {file}</item>",
        link("TN"),
        link("LRG"),
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)? + start;
    Some(&text[start..end])
}

/// Starts the pretend camera on 127.0.0.1 with a port of the system's choosing.
pub async fn spawn_sendto(config: SendToSim) -> SendToHandle {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind the pretend media server");
    let addr = listener.local_addr().expect("its address");
    let log: Arc<Mutex<Vec<String>>> = Arc::default();
    let seen = log.clone();
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(answer(stream, addr, config.clone(), seen.clone()));
        }
    });
    SendToHandle { addr, log, task }
}

async fn answer(
    stream: TcpStream,
    addr: SocketAddr,
    config: SendToSim,
    log: Arc<Mutex<Vec<String>>>,
) {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    if reader.read_line(&mut request).await.is_err() {
        return;
    }
    let mut length = 0usize;
    let mut action = String::new();
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header).await {
            Ok(0) | Err(_) => return,
            Ok(_) if header.trim_end().is_empty() => break,
            Ok(_) => {}
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("soapaction") {
                action = value.trim().trim_matches('"').to_owned();
            }
        }
    }
    let mut body = vec![0u8; length];
    if reader.read_exact(&mut body).await.is_err() {
        return;
    }
    let body = String::from_utf8_lossy(&body).into_owned();
    let mut parts = request.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let action = action.rsplit('#').next().unwrap_or("").to_owned();
    let (status, kind, bytes): (u16, &str, Vec<u8>) = match (method, path) {
        ("GET", "/DmsDescPush.xml") => {
            log.lock().unwrap().push(format!("GET {path}"));
            (200, "text/xml", b"<root><device><friendlyName>ZV-E10</friendlyName><manufacturer>Sony Corporation</manufacturer><modelName>ZV-E10</modelName></device></root>".to_vec())
        }
        ("POST", "/upnp/control/XPushList") => {
            log.lock().unwrap().push(action.clone());
            (200, "text/xml", b"<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body/></s:Envelope>".to_vec())
        }
        ("POST", "/upnp/control/ContentDirectory") => {
            let id = between(&body, "<ObjectID>", "</ObjectID>")
                .unwrap_or("")
                .to_owned();
            let start: usize = between(&body, "<StartingIndex>", "</StartingIndex>")
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
            log.lock().unwrap().push(format!("Browse {id} {start}"));
            let folder = format!("{}_1", config.root);
            let entries: Option<Vec<String>> = if id == config.root {
                Some(vec![format!(
                    "<container id=\"{folder}\" parentID=\"{}\" restricted=\"1\"><dc:title>2026-11-11</dc:title><upnp:class>object.container</upnp:class></container>",
                    config.root
                )])
            } else if id == folder {
                Some(vec![
                    item(addr, 1, "DSC00412.JPG", Some(original_jpeg().len())),
                    item(addr, 2, "DSC00413.ARW", None),
                    item(addr, 3, "C0007.MP4", Some(movie().len())),
                ])
            } else {
                None
            };
            match entries {
                Some(all) => {
                    let page: Vec<&String> = all.iter().skip(start).take(config.page).collect();
                    let didl = format!(
                        "<DIDL-Lite xmlns=\"urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" xmlns:upnp=\"urn:schemas-upnp-org:metadata-1-0/upnp/\">{}</DIDL-Lite>",
                        page.iter().map(|s| s.as_str()).collect::<String>()
                    );
                    let xml = format!(
                        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body><u:BrowseResponse xmlns:u=\"urn:schemas-upnp-org:service:ContentDirectory:1\"><Result>{}</Result><NumberReturned>{}</NumberReturned><TotalMatches>{}</TotalMatches><UpdateID>1</UpdateID></u:BrowseResponse></s:Body></s:Envelope>",
                        escape(&didl),
                        page.len(),
                        all.len()
                    );
                    (200, "text/xml", xml.into_bytes())
                }
                // UPnP's "no such object".
                None => (500, "text/xml", b"<s:Envelope><s:Body><s:Fault><detail><UPnPError><errorCode>701</errorCode></UPnPError></detail></s:Fault></s:Body></s:Envelope>".to_vec()),
            }
        }
        ("GET", _) => {
            log.lock().unwrap().push(format!("GET {path}"));
            match path {
                "/ORG_DSC00412.JPG" => (200, "image/jpeg", original_jpeg()),
                "/ORG_C0007.MP4" => (200, "video/mp4", movie()),
                // The RAW itself is not sent, as on the bodies that keep it.
                "/ORG_DSC00413.ARW" => (406, "text/plain", Vec::new()),
                p if p.starts_with("/TN_") => (200, "image/jpeg", small_jpeg(1)),
                p if p.starts_with("/LRG_") => (200, "image/jpeg", small_jpeg(2)),
                _ => (404, "text/plain", Vec::new()),
            }
        }
        _ => (404, "text/plain", Vec::new()),
    };
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    let mut stream = reader.into_inner();
    let _ = stream.write_all(head.as_bytes()).await;
    let _ = stream.write_all(&bytes).await;
    let _ = stream.shutdown().await;
}

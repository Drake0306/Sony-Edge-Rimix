//! The files on the camera, over Sony's "Send to Smartphone".
//!
//! Remote shooting (PTP/IP, port 15740) cannot list the card on bodies like
//! the ZV-E10: Sony's command reference marks GetStorageIDs,
//! GetObjectHandles and GetThumb as unsupported there. Files over Wi-Fi are
//! a different mode of the camera and a different protocol: with Send to
//! Smartphone chosen in its menu, the camera runs a UPnP media server on
//! its own network, and that is what Sony's phone app talks to.
//!
//! The protocol is not published. This follows `imaging-edge.py` of
//! schorschii/ImagingEdge4Linux, which reports it working on thirteen
//! bodies, and agrees with two other clients:
//!
//! - `GET /DmsDescPush.xml` (or `/dd.xml`) on port 64321 describes the server;
//! - a SOAP `X_TransferStart` to `/upnp/control/XPushList` puts
//!   "Transferring" on the camera's screen;
//! - a SOAP `Browse` to `/upnp/control/ContentDirectory` lists a folder:
//!   `PushRoot` holds what was chosen on the camera, `PhotoRoot` everything
//!   (on bodies that offer choosing on the phone). The answer carries a
//!   second XML document, DIDL-Lite, as text;
//! - every picture has several links: a thumbnail (`_TN`), previews (`_SM`,
//!   `_LRG`) and, where the camera sends it, the file itself with its size;
//! - `X_TransferEnd` ends it: the camera leaves the mode and its network.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use serde::{Deserialize, Serialize};
use std::io::Write;

use tokio::io::AsyncReadExt;
use url::Url;

use crate::discovery::http;
use crate::error::Error;
use crate::sony::capture::{check_filename, place};

/// The port Sony's cameras serve Send to Smartphone on.
pub const DEFAULT_PORT: u16 = 64321;
/// Where the server describes itself, the usual place first.
const DESCRIPTIONS: [&str; 2] = ["/DmsDescPush.xml", "/dd.xml"];
const CONTENT_DIRECTORY: &str = "/upnp/control/ContentDirectory";
const PUSH_LIST: &str = "/upnp/control/XPushList";
/// The folders everything hangs under: what was chosen on the camera, then
/// everything on the card.
const ROOTS: [&str; 2] = ["PushRoot", "PhotoRoot"];
/// How many entries one Browse asks for, as the reference client does.
const PAGE: u32 = 9999;
/// Most pages read of one folder.
const MAX_PAGES: u32 = 200;
/// How long the camera gets to answer a request's headers.
const ANSWER: Duration = Duration::from_secs(15);
/// How long a download may stand still.
const STALLED: Duration = Duration::from_secs(30);
/// The largest listing, thumbnail or preview taken into memory.
const MAX_SMALL: u64 = 24 * 1024 * 1024;
/// How much of a file is read at a time.
const CHUNK: usize = 64 * 1024;

/// A folder or a file the camera lists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    /// The folder's name, or the file's (`DSC00412.JPG`).
    pub title: String,
    pub folder: bool,
    /// When the picture was taken, as the camera writes it.
    pub date: Option<String>,
    /// A small picture of it, and a larger one to look at.
    pub thumb: Option<String>,
    pub preview: Option<String>,
    /// What an import fetches: the file itself where the camera sends it,
    /// else the largest picture of it the camera makes.
    pub file: Option<String>,
    pub bytes: Option<u64>,
    pub resolution: Option<String>,
    /// False when `file` is only a JPEG the camera made of the picture (a
    /// RAW on bodies that do not send RAW over Wi-Fi).
    pub original: bool,
}

/// One `res` of an item: a link and what the camera says about it.
#[derive(Debug, Default, Clone)]
struct Link {
    url: String,
    protocol: String,
    size: Option<u64>,
    resolution: Option<String>,
}

/// A camera in Send to Smartphone.
#[derive(Debug)]
pub struct SendTo {
    base: Url,
    /// The camera's name for itself.
    pub model: String,
    /// The folder everything hangs under: `PushRoot` or `PhotoRoot`.
    pub root: String,
}

fn bad_xml(e: impl std::fmt::Display) -> Error {
    Error::Protocol(format!("the camera sent XML that cannot be read: {e}"))
}

/// The text of the first element called `name`, whatever its namespace.
fn element_text(xml: &str, name: &str) -> Result<Option<String>, Error> {
    let mut reader = Reader::from_str(xml);
    let mut inside = false;
    let mut text = String::new();
    loop {
        match reader.read_event().map_err(bad_xml)? {
            Event::Start(e) if e.local_name().as_ref() == name.as_bytes() => inside = true,
            Event::Empty(e) if e.local_name().as_ref() == name.as_bytes() => {
                return Ok(Some(String::new()))
            }
            Event::Text(t) if inside => text.push_str(&t.unescape().map_err(bad_xml)?),
            Event::CData(t) if inside => text.push_str(&t.decode().map_err(bad_xml)?),
            Event::End(e) if inside && e.local_name().as_ref() == name.as_bytes() => {
                return Ok(Some(text.trim().to_owned()))
            }
            Event::Eof => return Ok(None),
            _ => {}
        }
    }
}

fn attribute(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        (a.key.local_name().as_ref() == name.as_bytes())
            .then(|| a.unescape_value().ok().map(|v| v.into_owned()))
            .flatten()
    })
}

/// The folders and files of one DIDL-Lite listing, in the camera's order.
pub fn parse_didl(didl: &str) -> Result<Vec<Entry>, Error> {
    struct Open {
        id: String,
        folder: bool,
        title: String,
        date: Option<String>,
        links: Vec<Link>,
    }
    let mut reader = Reader::from_str(didl);
    let mut out = Vec::new();
    let mut open: Option<Open> = None;
    // The element whose text is being read: "title", "date" or "res".
    let mut field: Option<&'static str> = None;
    let mut text = String::new();
    loop {
        let event = reader.read_event().map_err(bad_xml)?;
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.local_name();
                match name.as_ref() {
                    b"container" | b"item" => {
                        open = Some(Open {
                            id: attribute(e, "id").unwrap_or_default(),
                            folder: name.as_ref() == b"container",
                            title: String::new(),
                            date: None,
                            links: Vec::new(),
                        });
                    }
                    b"title" => field = Some("title"),
                    b"date" => field = Some("date"),
                    b"res" => {
                        field = Some("res");
                        if let Some(entry) = open.as_mut() {
                            entry.links.push(Link {
                                url: String::new(),
                                protocol: attribute(e, "protocolInfo").unwrap_or_default(),
                                size: attribute(e, "size").and_then(|s| s.parse().ok()),
                                resolution: attribute(e, "resolution"),
                            });
                        }
                    }
                    _ => {}
                }
                text.clear();
            }
            Event::Text(t) => text.push_str(&t.unescape().map_err(bad_xml)?),
            Event::CData(t) => text.push_str(&t.decode().map_err(bad_xml)?),
            Event::End(e) => {
                let name = e.local_name();
                let value = text.trim().to_owned();
                match (name.as_ref(), field.take(), open.as_mut()) {
                    (b"title", Some("title"), Some(entry)) => entry.title = value,
                    (b"date", Some("date"), Some(entry)) => entry.date = Some(value),
                    (b"res", Some("res"), Some(entry)) => {
                        if let Some(link) = entry.links.last_mut() {
                            link.url = value;
                        }
                    }
                    (b"container" | b"item", _, _) => {
                        if let Some(entry) = open.take() {
                            out.push(entry_of(
                                entry.id,
                                entry.title,
                                entry.folder,
                                entry.date,
                                entry.links,
                            ));
                        }
                    }
                    _ => {}
                }
                text.clear();
            }
            Event::Eof => return Ok(out),
            _ => {}
        }
    }
}

/// Chooses among an item's links as the reference client does: the file is
/// the link with the largest stated size; with no size stated (a RAW the
/// camera does not send) it is the large preview, else the last link.
fn entry_of(
    id: String,
    title: String,
    folder: bool,
    date: Option<String>,
    links: Vec<Link>,
) -> Entry {
    let links: Vec<Link> = links.into_iter().filter(|l| !l.url.is_empty()).collect();
    let marked = |mark: &str| links.iter().find(|l| l.protocol.contains(mark));
    let sized = links
        .iter()
        .filter(|l| l.size.is_some_and(|s| s > 0))
        .max_by_key(|l| l.size);
    let file = sized.or_else(|| marked("_LRG")).or(links.last());
    Entry {
        id,
        title,
        folder,
        date,
        thumb: marked("_TN").map(|l| l.url.clone()),
        preview: marked("_LRG")
            .or_else(|| marked("_SM"))
            .or_else(|| marked("_TN"))
            .map(|l| l.url.clone()),
        file: file.map(|l| l.url.clone()),
        bytes: file.and_then(|l| l.size),
        resolution: file.and_then(|l| l.resolution.clone()),
        original: sized.is_some(),
    }
}

fn soap(service: &str, action: &str, arguments: &str) -> String {
    format!(
        "<?xml version=\"1.0\"?>\
         <s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\">\
         <s:Body><u:{action} xmlns:u=\"{service}\">{arguments}</u:{action}></s:Body>\
         </s:Envelope>"
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

impl SendTo {
    /// Finds the camera's media server at `addr`, tells the camera a
    /// transfer starts, and finds the folder everything hangs under.
    pub async fn open(addr: SocketAddr) -> Result<SendTo, Error> {
        let base = Url::parse(&format!("http://{addr}/"))
            .map_err(|e| Error::Protocol(format!("bad address {addr}: {e}")))?;
        let mut model = None;
        let mut last = Error::Protocol("no description".into());
        for path in DESCRIPTIONS {
            let url = base.join(path).map_err(bad_xml)?;
            match http::send("GET", &url, &[], &[], ANSWER).await {
                Ok(reply) if reply.status == 200 => {
                    let xml = String::from_utf8_lossy(&reply.bytes(MAX_SMALL).await?).into_owned();
                    let name = element_text(&xml, "modelName")?
                        .or(element_text(&xml, "friendlyName")?)
                        .unwrap_or_default();
                    model = Some(name);
                    break;
                }
                Ok(reply) => last = Error::Protocol(format!("{url} answered {}", reply.status)),
                Err(e) => last = e,
            }
        }
        let Some(model) = model else {
            tracing::info!(%addr, error = %last, "no Send to Smartphone server answered");
            return Err(last);
        };
        let mut camera = SendTo {
            base,
            model,
            root: String::new(),
        };
        // Shows "Transferring" on the camera; only the chosen-on-camera
        // form knows it, so a refusal is no fault.
        if let Err(e) = camera.push_list("X_TransferStart", "").await {
            tracing::info!(error = %e, "the camera did not take X_TransferStart");
        }
        let mut refused = None;
        for root in ROOTS {
            match camera.page(root, 0).await {
                Ok(_) => {
                    camera.root = root.to_owned();
                    break;
                }
                Err(e) => refused = Some(e),
            }
        }
        if camera.root.is_empty() {
            return Err(refused.unwrap_or(Error::Protocol("the camera lists no folder".into())));
        }
        tracing::info!(%addr, model = %camera.model, root = %camera.root, "Send to Smartphone opened");
        Ok(camera)
    }

    async fn post(
        &self,
        path: &str,
        service: &str,
        action: &str,
        arguments: &str,
    ) -> Result<(u16, String), Error> {
        let url = self.base.join(path).map_err(bad_xml)?;
        let soap_action = format!("\"{service}#{action}\"");
        let headers = [
            ("SOAPACTION", soap_action.as_str()),
            ("Content-Type", "text/xml; charset=\"utf-8\""),
        ];
        let body = soap(service, action, arguments);
        let reply = http::send("POST", &url, &headers, body.as_bytes(), ANSWER).await?;
        let status = reply.status;
        let text = String::from_utf8_lossy(&reply.bytes(MAX_SMALL).await?).into_owned();
        Ok((status, text))
    }

    async fn push_list(&self, action: &str, arguments: &str) -> Result<(), Error> {
        let service = "urn:schemas-sony-com:service:XPushList:1";
        match self.post(PUSH_LIST, service, action, arguments).await? {
            (200, _) => Ok(()),
            (status, _) => Err(Error::Protocol(format!("{action} answered {status}"))),
        }
    }

    /// One page of a folder: its entries, how many came, how many there are.
    async fn page(&self, id: &str, start: u32) -> Result<(Vec<Entry>, u32, u32), Error> {
        let service = "urn:schemas-upnp-org:service:ContentDirectory:1";
        let arguments = format!(
            "<ObjectID>{}</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag>\
             <Filter>*</Filter><StartingIndex>{start}</StartingIndex>\
             <RequestedCount>{PAGE}</RequestedCount><SortCriteria></SortCriteria>",
            escape(id)
        );
        let (status, xml) = self
            .post(CONTENT_DIRECTORY, service, "Browse", &arguments)
            .await?;
        if status != 200 {
            return Err(Error::Protocol(format!(
                "the camera would not list {id} (answered {status})"
            )));
        }
        let didl = element_text(&xml, "Result")?.unwrap_or_default();
        let number = |name| -> Result<u32, Error> {
            Ok(element_text(&xml, name)?
                .and_then(|n| n.parse().ok())
                .unwrap_or(0))
        };
        let entries = if didl.is_empty() {
            Vec::new()
        } else {
            parse_didl(&didl)?
        };
        Ok((entries, number("NumberReturned")?, number("TotalMatches")?))
    }

    /// Everything directly inside the folder `id` (the root for `None`).
    pub async fn list(&self, id: Option<&str>) -> Result<Vec<Entry>, Error> {
        let id = id.unwrap_or(&self.root);
        let mut out = Vec::new();
        let mut start = 0;
        for _ in 0..MAX_PAGES {
            let (entries, returned, total) = self.page(id, start).await?;
            let came = returned.max(entries.len() as u32);
            out.extend(entries);
            start += came;
            if came == 0 || start >= total {
                break;
            }
        }
        tracing::info!(folder = %id, entries = out.len(), "listed a folder of the camera");
        Ok(out)
    }

    /// A link the camera gave, checked to lead back to the camera.
    fn link(&self, raw: &str) -> Result<Url, Error> {
        let url = self.base.join(raw).map_err(bad_xml)?;
        if url.scheme() != "http" || url.host_str() != self.base.host_str() {
            return Err(Error::Protocol(format!(
                "the camera gave a link that leads elsewhere: {raw}"
            )));
        }
        Ok(url)
    }

    /// A thumbnail or preview, whole.
    pub async fn picture(&self, link: &str) -> Result<Vec<u8>, Error> {
        let url = self.link(link)?;
        let reply = http::send("GET", &url, &[], &[], ANSWER).await?;
        if reply.status != 200 {
            return Err(Error::Protocol(format!(
                "the camera answered {} for a picture",
                reply.status
            )));
        }
        reply.bytes(MAX_SMALL).await
    }

    /// Downloads `link` into `dir` as `name` (or a free name like it),
    /// never over an existing file; `progress` hears the bytes so far and
    /// the size, when the camera states one. Returns where it was saved
    /// and how many bytes.
    pub async fn download(
        &self,
        link: &str,
        dir: &Path,
        name: &str,
        mut progress: impl FnMut(u64, Option<u64>),
    ) -> Result<(PathBuf, u64), Error> {
        let name = check_filename(name)?;
        let url = self.link(link)?;
        let mut reply = http::send("GET", &url, &[], &[], ANSWER).await?;
        if reply.status != 200 {
            // 406 is what bodies that keep RAW to themselves answer.
            return Err(Error::Protocol(format!(
                "the camera does not send {name} over Wi-Fi (answered {})",
                reply.status
            )));
        }
        let part = dir.join(format!(".{name}.rimix-part"));
        let total = reply.length;
        let result = async {
            let mut file = std::fs::File::create(&part)?;
            let mut buf = vec![0u8; CHUNK];
            let mut received = 0u64;
            loop {
                let n = tokio::time::timeout(STALLED, reply.body.read(&mut buf))
                    .await
                    .map_err(|_| Error::Timeout)??;
                if n == 0 {
                    break;
                }
                file.write_all(&buf[..n])?;
                received += n as u64;
                progress(received, total);
                if total.is_some_and(|t| received >= t) {
                    break;
                }
            }
            file.flush()?;
            // A file deleted from the card, or a link cut short.
            if total.is_some_and(|t| t != received) {
                return Err(Error::Protocol(format!(
                    "{name} stopped at {received} of {} bytes",
                    total.unwrap_or(0)
                )));
            }
            Ok(received)
        }
        .await;
        match result {
            Ok(received) => {
                let (path, _) = place(&part, dir, name)?;
                tracing::info!(file = %path.display(), bytes = received, "imported a file from the camera");
                Ok((path, received))
            }
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                tracing::warn!(file = name, error = %e, "importing a file from the camera failed");
                Err(e)
            }
        }
    }

    /// Tells the camera the transfer is over: it leaves Send to Smartphone
    /// and closes its network. Best effort.
    pub async fn close(&self) {
        if let Err(e) = self
            .push_list("X_TransferEnd", "<ErrCode>0</ErrCode>")
            .await
        {
            tracing::info!(error = %e, "the camera did not take X_TransferEnd");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped as the Sony listings the reference client parses: an item has
    // a thumbnail, two previews and the file with its size and resolution.
    const DIDL: &str = r#"<DIDL-Lite xmlns="urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:upnp="urn:schemas-upnp-org:metadata-1-0/upnp/">
<container id="PushRoot_1" parentID="PushRoot" restricted="1"><dc:title>2026-11-11</dc:title><upnp:class>object.container</upnp:class></container>
<item id="03_01_0000000001" parentID="PushRoot" restricted="1"><dc:title>DSC00412.JPG</dc:title><dc:date>2026-11-11T15:28:10</dc:date><upnp:class>object.item.imageItem.photo</upnp:class>
<res protocolInfo="http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_TN;DLNA.ORG_CI=1">http://192.168.122.1:64321/TN_DSC00412.JPG?%211234%21image%2Fjpeg%2A%21</res>
<res protocolInfo="http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_SM;DLNA.ORG_CI=1" resolution="640x424">http://192.168.122.1:64321/SM_DSC00412.JPG</res>
<res protocolInfo="http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_LRG;DLNA.ORG_CI=1" resolution="1616x1080">http://192.168.122.1:64321/LRG_DSC00412.JPG</res>
<res protocolInfo="http-get:*:image/jpeg:*" size="6100000" resolution="6000x4000">http://192.168.122.1:64321/ORG_DSC00412.JPG</res></item>
<item id="03_01_0000000002" parentID="PushRoot" restricted="1"><dc:title>DSC00413.ARW</dc:title>
<res protocolInfo="http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_TN;DLNA.ORG_CI=1">http://192.168.122.1:64321/TN_DSC00413.ARW</res>
<res protocolInfo="http-get:*:image/jpeg:DLNA.ORG_PN=JPEG_LRG;DLNA.ORG_CI=1">http://192.168.122.1:64321/LRG_DSC00413.ARW</res></item>
</DIDL-Lite>"#;

    #[test]
    fn a_listing_gives_folders_and_files_with_their_links() {
        let entries = parse_didl(DIDL).unwrap();
        assert_eq!(entries.len(), 3);
        let folder = &entries[0];
        assert!(folder.folder);
        assert_eq!(
            (folder.id.as_str(), folder.title.as_str()),
            ("PushRoot_1", "2026-11-11")
        );
        assert_eq!(folder.file, None);

        let jpeg = &entries[1];
        assert!(!jpeg.folder);
        assert_eq!(jpeg.title, "DSC00412.JPG");
        assert_eq!(jpeg.date.as_deref(), Some("2026-11-11T15:28:10"));
        // The escaped characters of a link come through as the camera means them.
        assert_eq!(
            jpeg.thumb.as_deref(),
            Some("http://192.168.122.1:64321/TN_DSC00412.JPG?%211234%21image%2Fjpeg%2A%21")
        );
        assert_eq!(
            jpeg.preview.as_deref(),
            Some("http://192.168.122.1:64321/LRG_DSC00412.JPG")
        );
        // The file is the link with a size: the picture as it was taken.
        assert_eq!(
            jpeg.file.as_deref(),
            Some("http://192.168.122.1:64321/ORG_DSC00412.JPG")
        );
        assert_eq!(jpeg.bytes, Some(6_100_000));
        assert_eq!(jpeg.resolution.as_deref(), Some("6000x4000"));
        assert!(jpeg.original);
    }

    #[test]
    fn a_raw_the_camera_keeps_to_itself_offers_its_large_preview() {
        let raw = &parse_didl(DIDL).unwrap()[2];
        assert_eq!(raw.title, "DSC00413.ARW");
        assert_eq!(
            raw.file.as_deref(),
            Some("http://192.168.122.1:64321/LRG_DSC00413.ARW")
        );
        assert_eq!(raw.bytes, None);
        assert!(!raw.original);
    }

    #[test]
    fn the_listing_inside_a_soap_answer_is_text_to_parse_again() {
        let soap = format!(
            "<s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\"><s:Body>\
             <u:BrowseResponse xmlns:u=\"urn:schemas-upnp-org:service:ContentDirectory:1\">\
             <Result>{}</Result><NumberReturned>3</NumberReturned><TotalMatches>3</TotalMatches>\
             </u:BrowseResponse></s:Body></s:Envelope>",
            escape(DIDL).replace('"', "&quot;")
        );
        let inner = element_text(&soap, "Result").unwrap().unwrap();
        assert_eq!(parse_didl(&inner).unwrap().len(), 3);
        assert_eq!(
            element_text(&soap, "TotalMatches").unwrap().as_deref(),
            Some("3")
        );
        assert_eq!(element_text(&soap, "Missing").unwrap(), None);
    }

    #[test]
    fn a_link_that_leads_away_from_the_camera_is_refused() {
        let camera = SendTo {
            base: Url::parse("http://192.168.122.1:64321/").unwrap(),
            model: String::new(),
            root: "PushRoot".into(),
        };
        assert!(camera
            .link("http://192.168.122.1:64321/ORG_DSC00412.JPG")
            .is_ok());
        assert!(camera.link("/TN_DSC00412.JPG").is_ok());
        assert!(camera.link("http://example.com/x.jpg").is_err());
        assert!(camera.link("file:///etc/passwd").is_err());
    }
}

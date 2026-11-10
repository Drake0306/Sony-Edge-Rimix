//! A minimal HTTP/1.1 GET, enough for UPnP descriptions: no redirects, no
//! chunked bodies, no TLS.

use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use url::Url;

use crate::error::Error;

/// How long a whole request may take.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Descriptions are a few kilobytes; refuse anything absurd.
const MAX_BODY: u64 = 1024 * 1024;

/// Fetches `url` and returns the body of a 200 response as text.
pub(crate) async fn get(url: &Url) -> Result<String, Error> {
    tokio::time::timeout(TIMEOUT, get_inner(url))
        .await
        .map_err(|_| Error::Timeout)?
}

async fn get_inner(url: &Url) -> Result<String, Error> {
    if url.scheme() != "http" {
        return Err(Error::Unsupported(
            "only http:// descriptions are supported",
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::Protocol(format!("{url} has no host")))?;
    let port = url.port_or_known_default().unwrap_or(80);
    let mut stream = TcpStream::connect((host.trim_matches(['[', ']']), port)).await?;
    let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await?;
    let status = line.split_whitespace().nth(1);
    if status != Some("200") {
        return Err(Error::Protocol(format!(
            "GET {url} answered {:?}",
            line.trim_end()
        )));
    }

    let mut content_length = None;
    loop {
        line.clear();
        if reader.read_line(&mut line).await? == 0 {
            return Err(Error::Protocol(format!("GET {url}: headers cut short")));
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = Some(value.parse::<u64>().map_err(|_| {
                Error::Protocol(format!("GET {url}: bad Content-Length {value:?}"))
            })?);
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && !value.eq_ignore_ascii_case("identity")
        {
            return Err(Error::Unsupported("chunked HTTP bodies"));
        }
    }

    let mut body = Vec::new();
    match content_length {
        Some(len) if len > MAX_BODY => {
            return Err(Error::Protocol(format!("GET {url}: body of {len} bytes")))
        }
        Some(len) => {
            body.resize(len as usize, 0);
            reader.read_exact(&mut body).await?;
        }
        None => {
            reader.take(MAX_BODY).read_to_end(&mut body).await?;
        }
    }
    String::from_utf8(body).map_err(|_| Error::Protocol(format!("GET {url}: body is not UTF-8")))
}

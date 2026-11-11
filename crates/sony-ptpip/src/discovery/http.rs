//! A minimal HTTP/1.1 GET, enough for UPnP descriptions: no redirects, no
//! chunked bodies, no TLS.

use std::time::Duration;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use url::Url;

use crate::error::Error;

/// How long a whole request may take.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Descriptions are a few kilobytes; refuse anything absurd.
const MAX_BODY: u64 = 1024 * 1024;
/// Longest status or header line, terminator included.
const MAX_LINE: usize = 8 * 1024;
/// Largest status line plus header section.
const MAX_HEADER_BYTES: usize = 64 * 1024;
/// Most header lines accepted.
const MAX_HEADERS: usize = 100;

/// Fetches `url` and returns the body of a 200 response as text.
pub(crate) async fn get(url: &Url) -> Result<String, Error> {
    get_with_timeout(url, TIMEOUT).await
}

async fn get_with_timeout(url: &Url, timeout: Duration) -> Result<String, Error> {
    tokio::time::timeout(timeout, get_inner(url))
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
    let mut header_budget = MAX_HEADER_BYTES;
    let line = read_line_capped(&mut reader, &mut header_budget)
        .await?
        .ok_or_else(|| Error::Protocol(format!("GET {url}: no response")))?;
    if line.split_whitespace().nth(1) != Some("200") {
        return Err(Error::Protocol(format!("GET {url} answered {line:?}")));
    }

    let mut content_length = None;
    let mut headers = 0;
    loop {
        let Some(header) = read_line_capped(&mut reader, &mut header_budget).await? else {
            return Err(Error::Protocol(format!("GET {url}: headers cut short")));
        };
        if header.is_empty() {
            break;
        }
        headers += 1;
        if headers > MAX_HEADERS {
            return Err(Error::Protocol(format!(
                "GET {url}: more than {MAX_HEADERS} headers"
            )));
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

    let too_big = || Error::Protocol(format!("GET {url}: body over {MAX_BODY} bytes"));
    let mut body = Vec::new();
    match content_length {
        Some(len) if len > MAX_BODY => return Err(too_big()),
        Some(len) => {
            body.resize(len as usize, 0);
            reader.read_exact(&mut body).await?;
        }
        None => {
            reader.take(MAX_BODY + 1).read_to_end(&mut body).await?;
            if body.len() as u64 > MAX_BODY {
                return Err(too_big());
            }
        }
    }
    String::from_utf8(body).map_err(|_| Error::Protocol(format!("GET {url}: body is not UTF-8")))
}

/// A response whose body has not been read yet.
pub(crate) struct Reply {
    pub status: u16,
    /// The Content-Length, when the server gave one.
    pub length: Option<u64>,
    pub body: BufReader<TcpStream>,
}

/// Sends one request and reads the response's status and headers; the body
/// is left to the caller, to stream or to cap. `Connection: close` is asked
/// for, so a body with no length ends with the connection. `timeout` covers
/// connecting and the headers only.
pub(crate) async fn send(
    method: &str,
    url: &Url,
    headers: &[(&str, &str)],
    body: &[u8],
    timeout: Duration,
) -> Result<Reply, Error> {
    tokio::time::timeout(timeout, send_inner(method, url, headers, body))
        .await
        .map_err(|_| Error::Timeout)?
}

async fn send_inner(
    method: &str,
    url: &Url,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<Reply, Error> {
    if url.scheme() != "http" {
        return Err(Error::Unsupported("only http:// is supported"));
    }
    let host = url
        .host_str()
        .ok_or_else(|| Error::Protocol(format!("{url} has no host")))?;
    let port = url.port_or_known_default().unwrap_or(80);
    let mut stream = TcpStream::connect((host.trim_matches(['[', ']']), port)).await?;
    let path = &url[url::Position::BeforePath..url::Position::AfterQuery];
    let mut request =
        format!("{method} {path} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if !body.is_empty() || method != "GET" {
        request.push_str(&format!("Content-Length: {}\r\n", body.len()));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).await?;
    stream.write_all(body).await?;

    let mut reader = BufReader::new(stream);
    let mut header_budget = MAX_HEADER_BYTES;
    let line = read_line_capped(&mut reader, &mut header_budget)
        .await?
        .ok_or_else(|| Error::Protocol(format!("{method} {url}: no response")))?;
    let status = line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or_else(|| Error::Protocol(format!("{method} {url} answered {line:?}")))?;
    let mut length = None;
    let mut seen = 0;
    loop {
        let Some(header) = read_line_capped(&mut reader, &mut header_budget).await? else {
            return Err(Error::Protocol(format!(
                "{method} {url}: headers cut short"
            )));
        };
        if header.is_empty() {
            break;
        }
        seen += 1;
        if seen > MAX_HEADERS {
            return Err(Error::Protocol(format!(
                "{method} {url}: more than {MAX_HEADERS} headers"
            )));
        }
        let Some((name, value)) = header.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.parse::<u64>().map_err(|_| {
                Error::Protocol(format!("{method} {url}: bad Content-Length {value:?}"))
            })?);
        } else if name.eq_ignore_ascii_case("transfer-encoding")
            && !value.eq_ignore_ascii_case("identity")
        {
            return Err(Error::Unsupported("chunked HTTP bodies"));
        }
    }
    Ok(Reply {
        status,
        length,
        body: reader,
    })
}

impl Reply {
    /// The whole body, refused beyond `max` bytes.
    pub(crate) async fn bytes(mut self, max: u64) -> Result<Vec<u8>, Error> {
        let too_big = || Error::Protocol(format!("HTTP body over {max} bytes"));
        let mut body = Vec::new();
        match self.length {
            Some(len) if len > max => return Err(too_big()),
            Some(len) => {
                body.resize(len as usize, 0);
                self.body.read_exact(&mut body).await?;
            }
            None => {
                (&mut self.body)
                    .take(max + 1)
                    .read_to_end(&mut body)
                    .await?;
                if body.len() as u64 > max {
                    return Err(too_big());
                }
            }
        }
        Ok(body)
    }
}

/// Reads one CRLF- or LF-terminated line without its terminator, charging
/// its length to `budget`. `None` means the stream ended before any byte.
/// A line over `MAX_LINE` or over the remaining budget is an error.
async fn read_line_capped(
    reader: &mut (impl AsyncBufRead + Unpin),
    budget: &mut usize,
) -> Result<Option<String>, Error> {
    let limit = MAX_LINE.min(*budget);
    let mut line = Vec::new();
    let n = reader
        .take(limit as u64)
        .read_until(b'\n', &mut line)
        .await?;
    if n == 0 {
        return Ok(None);
    }
    if line.last() != Some(&b'\n') {
        if n == limit {
            return Err(Error::Protocol(format!(
                "HTTP header line over {MAX_LINE} bytes or header section over \
                 {MAX_HEADER_BYTES} bytes"
            )));
        }
        // The stream ended mid-line.
        return Err(Error::Protocol("HTTP headers cut short".into()));
    }
    *budget -= n;
    let text =
        String::from_utf8(line).map_err(|_| Error::Protocol("HTTP header is not UTF-8".into()))?;
    Ok(Some(text.trim_end_matches(['\r', '\n']).to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    /// Serves `response` to one request on loopback, then closes.
    async fn serve(response: Vec<u8>) -> Url {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut buf = [0u8; 512];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut buf).await {
                    Ok(0) | Err(_) => return,
                    Ok(n) => request.extend_from_slice(&buf[..n]),
                }
            }
            let _ = stream.write_all(&response).await;
        });
        Url::parse(&format!("http://{addr}/dd.xml")).unwrap()
    }

    async fn get_response(response: impl Into<Vec<u8>>) -> Result<String, Error> {
        get(&serve(response.into()).await).await
    }

    #[tokio::test]
    async fn returns_body_by_content_length() {
        let body = get_response("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello")
            .await
            .unwrap();
        assert_eq!(body, "hello");
    }

    #[tokio::test]
    async fn returns_body_read_to_eof() {
        let body = get_response("HTTP/1.1 200 OK\r\n\r\n<root/>")
            .await
            .unwrap();
        assert_eq!(body, "<root/>");
    }

    #[tokio::test]
    async fn non_200_status_is_an_error() {
        let err = get_response("HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn oversize_content_length_is_an_error() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY + 1
        );
        let err = get_response(response).await.unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn truncated_body_is_an_error() {
        let err = get_response("HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Io(_)), "{err:?}");
    }

    #[tokio::test]
    async fn chunked_body_is_refused() {
        let err = get_response(
            "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        )
        .await
        .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)), "{err:?}");
    }

    #[tokio::test]
    async fn headers_cut_short_are_an_error() {
        let err = get_response("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
        let err = get_response("HTTP/1.1 200 OK\r\nContent-Le")
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn header_line_over_8_kib_is_an_error() {
        let response = format!(
            "HTTP/1.1 200 OK\r\nX-Long: {}\r\n\r\n",
            "a".repeat(MAX_LINE)
        );
        let err = get_response(response).await.unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn status_line_over_8_kib_is_an_error() {
        let response = format!("HTTP/1.1 200 {}\r\n\r\n", "O".repeat(MAX_LINE));
        let err = get_response(response).await.unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn header_section_over_64_kib_is_an_error() {
        // 90 headers of about 1 KiB each: under the count cap, over the byte cap.
        let mut response = String::from("HTTP/1.1 200 OK\r\n");
        for i in 0..90 {
            response.push_str(&format!("X-H{i}: {}\r\n", "b".repeat(1000)));
        }
        response.push_str("\r\n");
        let err = get_response(response).await.unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn more_than_100_headers_is_an_error() {
        let mut response = String::from("HTTP/1.1 200 OK\r\n");
        for i in 0..=MAX_HEADERS {
            response.push_str(&format!("X-H{i}: 1\r\n"));
        }
        response.push_str("Content-Length: 0\r\n\r\n");
        let err = get_response(response).await.unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn eof_body_over_the_cap_is_an_error() {
        let mut response = b"HTTP/1.1 200 OK\r\n\r\n".to_vec();
        response.resize(response.len() + MAX_BODY as usize + 1, b'x');
        let err = get_response(response).await.unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[tokio::test]
    async fn silent_server_times_out() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}/dd.xml", listener.local_addr().unwrap())).unwrap();
        // The listener accepts (via the backlog) but never answers.
        let err = get_with_timeout(&url, Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Timeout), "{err:?}");
        drop(listener);
    }
}

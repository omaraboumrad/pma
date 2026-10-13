//! Just enough HTTP/1.1 for one local user: one request per connection,
//! headers parsed by `httparse`, sizes capped, and the same security headers
//! on every response.

use std::io::{Read, Write};
use std::net::TcpStream;

/// Headers past this are refused: a browser sends a few hundred bytes.
const MAX_HEAD: usize = 16 * 1024;
/// The largest body an action takes: a note or rework feedback.
const MAX_BODY: usize = 64 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    pub query: String,
    headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    /// A header's value; names compare without case.
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// One `name=value` pair of the query string, percent-decoded: a
    /// project's name may hold any character.
    pub fn param(&self, name: &str) -> Option<String> {
        self.query
            .split('&')
            .filter_map(|p| p.split_once('='))
            .find(|(n, _)| *n == name)
            .map(|(_, v)| decode(v))
    }

    pub fn cookie(&self, name: &str) -> Option<&str> {
        self.header("cookie")?
            .split(';')
            .filter_map(|c| c.trim().split_once('='))
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v)
    }
}

/// Reads one request. The error is the status to answer with.
pub fn read(stream: &mut TcpStream) -> Result<Request, u16> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
        if buf.len() > MAX_HEAD {
            return Err(431);
        }
        let n = stream.read(&mut chunk).map_err(|_| 408u16)?;
        if n == 0 {
            return Err(400);
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let mut headers = [httparse::EMPTY_HEADER; 64];
    let mut req = httparse::Request::new(&mut headers);
    match req.parse(&buf[..head_end]) {
        Ok(httparse::Status::Complete(_)) => {}
        _ => return Err(400),
    }
    let target = req.path.ok_or(400u16)?;
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers: Vec<(String, String)> = req
        .headers
        .iter()
        .map(|h| {
            (
                h.name.to_string(),
                String::from_utf8_lossy(h.value).into_owned(),
            )
        })
        .collect();
    let method = req.method.ok_or(400u16)?.to_string();
    let length = match headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
    {
        None => 0,
        Some((_, v)) => v.trim().parse::<usize>().map_err(|_| 400u16)?,
    };
    if length > MAX_BODY {
        return Err(413);
    }
    let mut body = buf[head_end..].to_vec();
    while body.len() < length {
        let n = stream.read(&mut chunk).map_err(|_| 408u16)?;
        if n == 0 {
            return Err(400);
        }
        body.extend_from_slice(&chunk[..n]);
    }
    body.truncate(length);
    Ok(Request {
        method,
        path: path.to_string(),
        query: query.to_string(),
        headers,
        body,
    })
}

/// `%XX` escapes and `+` for space, as a browser encodes a query value. A
/// malformed escape is kept as written.
fn decode(v: &str) -> String {
    let b = v.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                    Ok(x) => {
                        out.push(x);
                        i += 2;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
    pub extra: Vec<(&'static str, String)>,
}

impl Response {
    pub fn new(status: u16, content_type: &'static str, body: impl Into<Vec<u8>>) -> Response {
        Response {
            status,
            content_type,
            body: body.into(),
            extra: Vec::new(),
        }
    }

    pub fn json(status: u16, value: &serde_json::Value) -> Response {
        Response::new(status, "application/json", value.to_string())
    }

    pub fn status(status: u16) -> Response {
        Response::new(status, "text/plain; charset=utf-8", reason(status))
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        413 => "Content Too Large",
        415 => "Unsupported Media Type",
        421 => "Misdirected Request",
        431 => "Request Header Fields Too Large",
        _ => "Error",
    }
}

/// Every response says: run only our own script and style, connect only
/// back here, never frame this page, and keep nothing in a cache.
pub const SECURITY: [(&str, &str); 5] = [
    (
        "Content-Security-Policy",
        "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; \
         base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
    ),
    ("X-Content-Type-Options", "nosniff"),
    ("Referrer-Policy", "no-referrer"),
    ("Cache-Control", "no-store"),
    ("X-Frame-Options", "DENY"),
];

fn head(status: u16, content_type: &str) -> String {
    let mut out = format!(
        "HTTP/1.1 {status} {}\r\nContent-Type: {content_type}\r\n",
        reason(status)
    );
    for (n, v) in SECURITY {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out
}

pub fn write(stream: &mut TcpStream, r: &Response) -> std::io::Result<()> {
    let mut out = head(r.status, r.content_type);
    for (n, v) in &r.extra {
        out.push_str(&format!("{n}: {v}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        r.body.len()
    ));
    stream.write_all(out.as_bytes())?;
    stream.write_all(&r.body)?;
    stream.flush()
}

/// Starts a server-sent event stream; the caller writes events after it.
pub fn start_events(stream: &mut TcpStream) -> std::io::Result<()> {
    let mut out = head(200, "text/event-stream");
    out.push_str("Connection: keep-alive\r\n\r\n");
    stream.write_all(out.as_bytes())?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_values_are_decoded() {
        assert_eq!(super::decode("socket.io"), "socket.io");
        assert_eq!(super::decode("a%20b+c%2Fd"), "a b c/d");
        assert_eq!(super::decode("100%"), "100%");
        assert_eq!(super::decode("%zz"), "%zz");
    }
}

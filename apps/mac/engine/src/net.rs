//! A small blocking HTTP layer for the server client, URL reading, YouTube and
//! transcription. Everything goes through the [`Http`] trait so tests can answer
//! requests without a network; [`UreqHttp`] is the real one.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

/// One request. Header names are compared case-insensitively.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
    /// The whole request, body included. None means no limit.
    pub timeout: Option<Duration>,
}

impl Request {
    pub fn new(method: &str, url: impl Into<String>) -> Self {
        Request { method: method.into(), url: url.into(), ..Default::default() }
    }
    pub fn get(url: impl Into<String>) -> Self {
        Self::new("GET", url)
    }
    pub fn post(url: impl Into<String>) -> Self {
        Self::new("POST", url)
    }
    pub fn header(mut self, name: &str, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn body(mut self, body: Vec<u8>) -> Self {
        self.body = Some(body);
        self
    }
    pub fn json(self, value: &serde_json::Value) -> Self {
        self.header("Content-Type", "application/json").body(value.to_string().into_bytes())
    }
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
    pub fn header_value(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
    /// The body as text, for tests and logs.
    pub fn body_text(&self) -> String {
        self.body.as_deref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default()
    }
}

/// A response whose body is read as it arrives, so event streams can be handled line by line.
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Box<dyn Read + Send>,
}

impl std::fmt::Debug for Response {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Response").field("status", &self.status).field("headers", &self.headers).finish_non_exhaustive()
    }
}

impl Response {
    /// A canned response, for tests.
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Response { status, headers: Vec::new(), body: Box::new(std::io::Cursor::new(body.into())) }
    }
    pub fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        find_header(&self.headers, name)
    }
    pub fn bytes(mut self) -> std::io::Result<Vec<u8>> {
        let mut out = Vec::new();
        self.body.read_to_end(&mut out)?;
        Ok(out)
    }
    /// The body as text, replacing invalid UTF-8.
    pub fn text(self) -> std::io::Result<String> {
        Ok(String::from_utf8_lossy(&self.bytes()?).into_owned())
    }
}

fn find_header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Why a request got no response at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Transport {
    Timeout,
    Failed(String),
}

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Transport::Timeout => write!(f, "timed out"),
            Transport::Failed(m) => write!(f, "{m}"),
        }
    }
}

/// Sends a request. Error statuses are responses, not errors.
pub trait Http: Send + Sync {
    fn send(&self, request: Request) -> Result<Response, Transport>;
}

pub type SharedHttp = Arc<dyn Http>;

impl<F> Http for F
where
    F: Fn(Request) -> Result<Response, Transport> + Send + Sync,
{
    fn send(&self, request: Request) -> Result<Response, Transport> {
        self(request)
    }
}

/// The real network, through ureq.
#[derive(Clone)]
pub struct UreqHttp {
    agent: ureq::Agent,
}

impl Default for UreqHttp {
    fn default() -> Self {
        let config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .user_agent("OpenNotebook-Mac")
            .build();
        UreqHttp { agent: ureq::Agent::new_with_config(config) }
    }
}

impl UreqHttp {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn shared() -> SharedHttp {
        Arc::new(Self::default())
    }
}

impl Http for UreqHttp {
    fn send(&self, request: Request) -> Result<Response, Transport> {
        let mut builder = ureq::http::Request::builder().method(request.method.as_str()).uri(request.url.as_str());
        for (k, v) in &request.headers {
            builder = builder.header(k.as_str(), v.as_str());
        }
        let built = builder.body(request.body.unwrap_or_default()).map_err(|e| Transport::Failed(e.to_string()))?;
        let configured = self.agent.configure_request(built).timeout_global(request.timeout).build();
        let response = self.agent.run(configured).map_err(|e| match e {
            ureq::Error::Timeout(_) => Transport::Timeout,
            other => Transport::Failed(other.to_string()),
        })?;
        let status = response.status().as_u16();
        let headers = response
            .headers()
            .iter()
            .map(|(k, v)| (k.as_str().to_string(), String::from_utf8_lossy(v.as_bytes()).into_owned()))
            .collect();
        let reader = response.into_body().into_reader();
        Ok(Response { status, headers, body: Box::new(TimeoutReader(reader)) })
    }
}

/// Reports a body read that ran out of time as `TimedOut`, so callers can tell it apart.
struct TimeoutReader<R>(R);

impl<R: Read> Read for TimeoutReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf).map_err(|e| {
            let timed_out = e.get_ref().and_then(|inner| inner.downcast_ref::<ureq::Error>()).is_some_and(|u| matches!(u, ureq::Error::Timeout(_)));
            if timed_out { std::io::Error::new(std::io::ErrorKind::TimedOut, e) } else { e }
        })
    }
}

/// A multipart/form-data body, built in memory.
#[derive(Debug)]
pub struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Default for Multipart {
    fn default() -> Self {
        Multipart { boundary: format!("----OpenNotebook{}", uuid::Uuid::new_v4().simple()), body: Vec::new() }
    }
}

impl Multipart {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(mut self, name: &str, value: &str) -> Self {
        self.start(name, None, None);
        self.body.extend_from_slice(value.as_bytes());
        self.body.extend_from_slice(b"\r\n");
        self
    }

    /// A file part. An empty mime is sent as application/octet-stream, as browsers do.
    pub fn file(mut self, name: &str, file_name: &str, mime: &str, data: &[u8]) -> Self {
        let mime = if mime.is_empty() { "application/octet-stream" } else { mime };
        self.start(name, Some(file_name), Some(mime));
        self.body.extend_from_slice(data);
        self.body.extend_from_slice(b"\r\n");
        self
    }

    fn start(&mut self, name: &str, file_name: Option<&str>, mime: Option<&str>) {
        let mut head = format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"", self.boundary, quote(name));
        if let Some(f) = file_name {
            head.push_str(&format!("; filename=\"{}\"", quote(f)));
        }
        head.push_str("\r\n");
        if let Some(m) = mime {
            head.push_str(&format!("Content-Type: {m}\r\n"));
        }
        head.push_str("\r\n");
        self.body.extend_from_slice(head.as_bytes());
    }

    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    /// The finished body.
    pub fn finish(mut self) -> (String, Vec<u8>) {
        self.body.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (self.content_type(), self.body)
    }
}

/// Escapes a form-data name the way browsers do (WHATWG: quotes and newlines are percent-encoded).
fn quote(s: &str) -> String {
    s.replace('"', "%22").replace('\r', "%0D").replace('\n', "%0A")
}

/// `encodeURIComponent`.
pub fn encode_component(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `decodeURIComponent`; None when it would throw.
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A query string as `URLSearchParams` writes it.
pub fn query_string<'a>(pairs: impl IntoIterator<Item = (&'a str, String)>) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in pairs {
        s.append_pair(k, &v);
    }
    s.finish()
}

/// Reads a server-sent-events body and calls `on_data` with each event's data, as `readSse` does.
/// Stops early when `on_data` returns false.
pub fn read_sse(body: &mut dyn Read, mut on_data: impl FnMut(&str) -> crate::Result<bool>) -> crate::Result<()> {
    let mut reader = std::io::BufReader::new(body);
    let mut data: Vec<String> = Vec::new();
    let mut raw = Vec::new();
    loop {
        raw.clear();
        let n = std::io::BufRead::read_until(&mut reader, b'\n', &mut raw)?;
        let eof = n == 0;
        let line = String::from_utf8_lossy(&raw);
        let line = line.trim_end_matches('\n').trim_end_matches('\r');
        if eof || line.is_empty() {
            if !data.is_empty() && !on_data(&data.join("\n"))? {
                return Ok(());
            }
            data.clear();
            if eof {
                return Ok(());
            }
        } else if let Some(rest) = line.strip_prefix("data:") {
            data.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_component_matches_javascript() {
        assert_eq!(encode_component("notebook:abc/1 ü"), "notebook%3Aabc%2F1%20%C3%BC");
        assert_eq!(encode_component("a-b_c.d!~*'()"), "a-b_c.d!~*'()");
    }

    #[test]
    fn query_string_matches_url_search_params() {
        assert_eq!(query_string([("notebook_id", "notebook:abc".into()), ("q", "a b".into())]), "notebook_id=notebook%3Aabc&q=a+b");
    }

    #[test]
    fn multipart_has_boundaries_and_file_headers() {
        let (ty, body) = Multipart::new().text("type", "upload").file("file", "a\"b.pdf", "", b"%PDF").finish();
        let boundary = ty.strip_prefix("multipart/form-data; boundary=").unwrap();
        let body = String::from_utf8(body).unwrap();
        assert!(body.starts_with(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"type\"\r\n\r\nupload\r\n")));
        assert!(body.contains("name=\"file\"; filename=\"a%22b.pdf\"\r\nContent-Type: application/octet-stream\r\n\r\n%PDF\r\n"));
        assert!(body.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn sse_joins_data_lines_and_flushes_at_the_end() {
        let mut body = std::io::Cursor::new(b"event: x\r\ndata: one\r\ndata: two\r\n\r\ndata:three".to_vec());
        let mut seen = Vec::new();
        read_sse(&mut body, |d| {
            seen.push(d.to_string());
            Ok(true)
        })
        .unwrap();
        assert_eq!(seen, ["one\ntwo", "three"]);
    }
}

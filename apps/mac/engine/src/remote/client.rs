//! Requests to an Open Notebook server: base URL handling, the password header,
//! timeouts and FastAPI error messages. Port of `createClient`'s `request` in
//! `packages/api-client/src/client.ts`.

use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Error;
use crate::backend::Result;
use crate::net::{Multipart, Request, Response, SharedHttp, Transport, encode_component, percent_decode, query_string};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

/// Strips trailing slashes and a trailing /api so either form of the URL works,
/// and adds https:// to a bare host.
pub fn normalize_base_url(raw: &str) -> String {
    let mut url = raw.trim().trim_end_matches('/').to_string();
    if url.to_ascii_lowercase().ends_with("/api") {
        url.truncate(url.len() - 4);
    }
    let lower = url.to_ascii_lowercase();
    if !url.is_empty() && !lower.starts_with("http://") && !lower.starts_with("https://") {
        url = format!("https://{url}");
    }
    url
}

/// The message of a failed response: FastAPI's `detail`, else the body, else the status.
pub fn error_message(status: u16, body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(map)) => {
            if let Some(detail) = map.get("detail") {
                if let Some(s) = detail.as_str() {
                    return s.to_string();
                }
                if let Some(msg) = detail.as_array().and_then(|a| a.first()).and_then(|d| d.get("msg")).filter(|m| truthy(m)) {
                    return msg.as_str().map(str::to_string).unwrap_or_else(|| msg.to_string());
                }
            }
        }
        Ok(Value::String(s)) if !s.trim().is_empty() => return s.trim().chars().take(200).collect(),
        Ok(_) => {}
        Err(_) if !body.trim().is_empty() => return body.trim().chars().take(200).collect(),
        Err(_) => {}
    }
    format!("Request failed with status {status}")
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    }
}

/// What a request sends.
pub enum Body {
    None,
    Json(Value),
    Form(Multipart),
}

/// The original file of a source.
pub struct Download {
    pub data: Vec<u8>,
    pub name: Option<String>,
    pub mime: String,
}

/// A connection to one server. Cheap to clone.
#[derive(Clone)]
pub struct Client {
    base_url: String,
    password: Option<String>,
    http: SharedHttp,
    timeout: Duration,
}

impl Client {
    pub fn new(base_url: &str, password: Option<&str>, http: SharedHttp) -> Self {
        Client {
            base_url: normalize_base_url(base_url),
            password: password.map(str::trim).filter(|p| !p.is_empty()).map(str::to_string),
            http,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    /// Absolute URL for a server-relative path such as an audio-access URL.
    pub fn resolve_url(&self, path: &str) -> String {
        let lower = path.to_ascii_lowercase();
        if lower.starts_with("http://") || lower.starts_with("https://") {
            return path.to_string();
        }
        format!("{}{}{}", self.base_url, if path.starts_with('/') { "" } else { "/" }, path)
    }

    /// `/api{path}` with the query appended; None values are skipped.
    pub fn url(&self, path: &str, query: &[(&str, Option<String>)]) -> String {
        let mut url = format!("{}/api{}", self.base_url, path);
        let qs = query_string(query.iter().filter_map(|(k, v)| v.clone().map(|v| (*k, v))));
        if !qs.is_empty() {
            url.push('?');
            url.push_str(&qs);
        }
        url
    }

    /// Sends a request and returns the response if it succeeded.
    pub fn send(&self, method: &str, path: &str, query: &[(&str, Option<String>)], body: Body, timeout: Option<Duration>) -> Result<Response> {
        let mut req = Request::new(method, self.url(path, query)).header("Accept", "application/json");
        match body {
            Body::None => {}
            Body::Json(v) => req = req.json(&v),
            Body::Form(form) => {
                let (ty, bytes) = form.finish();
                req = req.header("Content-Type", ty).body(bytes);
            }
        }
        if let Some(p) = &self.password {
            req = req.header("Authorization", format!("Bearer {p}"));
        }
        let limit = timeout.unwrap_or(self.timeout);
        if !limit.is_zero() {
            req = req.timeout(limit);
        }
        let response = self.http.send(req).map_err(|e| match e {
            Transport::Timeout => Error::Network("The server took too long to respond".into()),
            Transport::Failed(_) => Error::Network(format!("Could not reach {}", self.base_url)),
        })?;
        if response.ok() {
            return Ok(response);
        }
        let status = response.status;
        let text = read_text(response)?;
        Err(Error::Server { status, message: error_message(status, &text) })
    }

    /// A JSON response; an empty body reads as null.
    pub fn json<T: DeserializeOwned>(&self, method: &str, path: &str, query: &[(&str, Option<String>)], body: Body, timeout: Option<Duration>) -> Result<T> {
        let text = read_text(self.send(method, path, query, body, timeout)?)?;
        Ok(serde_json::from_str(if text.is_empty() { "null" } else { &text })?)
    }

    pub fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, Option<String>)]) -> Result<T> {
        self.json("GET", path, query, Body::None, None)
    }

    pub fn post<T: DeserializeOwned>(&self, path: &str, body: Option<Value>) -> Result<T> {
        self.json("POST", path, &[], body.map_or(Body::None, Body::Json), None)
    }

    pub fn put<T: DeserializeOwned>(&self, path: &str, body: Value) -> Result<T> {
        self.json("PUT", path, &[], Body::Json(body), None)
    }

    /// A request whose answer is not needed.
    pub fn call(&self, method: &str, path: &str, query: &[(&str, Option<String>)], body: Option<Value>) -> Result<()> {
        read_text(self.send(method, path, query, body.map_or(Body::None, Body::Json), None)?)?;
        Ok(())
    }

    pub fn delete(&self, path: &str) -> Result<()> {
        self.call("DELETE", path, &[], None)
    }

    /// A file download with its name from Content-Disposition.
    pub fn download(&self, path: &str) -> Result<Download> {
        let response = self.send("GET", path, &[], Body::None, None)?;
        let name = response.header("content-disposition").and_then(disposition_name);
        let mime = response.header("content-type").unwrap_or_default().to_string();
        let data = response.bytes().map_err(read_error)?;
        Ok(Download { data, name, mime })
    }
}

/// `encodeURIComponent` for a path segment.
pub fn seg(s: &str) -> String {
    encode_component(s)
}

pub fn read_text(response: Response) -> Result<String> {
    response.text().map_err(read_error)
}

/// A body read that failed partway.
pub fn read_error(e: std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::TimedOut { Error::Network("The server took too long to respond".into()) } else { Error::Network(e.to_string()) }
}

/// The file name in a Content-Disposition header, matching
/// `/filename\*?=(?:UTF-8'')?"?([^";]+)"?/i`, percent-decoded.
pub fn disposition_name(header: &str) -> Option<String> {
    let lower = header.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find("filename") {
        let mut rest = &header[from + i + "filename".len()..];
        from += i + "filename".len();
        rest = rest.strip_prefix('*').unwrap_or(rest);
        let Some(after) = rest.strip_prefix('=') else { continue };
        let after = if after.len() >= 7 && after[..7].eq_ignore_ascii_case("UTF-8''") { &after[7..] } else { after };
        let after = after.strip_prefix('"').unwrap_or(after);
        let end = after.find(['"', ';']).unwrap_or(after.len());
        if end == 0 {
            continue;
        }
        let raw = &after[..end];
        return Some(percent_decode(raw).unwrap_or_else(|| raw.to_string()));
    }
    None
}

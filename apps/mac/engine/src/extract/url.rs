//! Downloads a page, PDF or YouTube video and extracts its text. Port of
//! `makeUrlExtractor` and `readYouTube` in `apps/desktop/src/platform/extract.ts`.

use std::time::Duration;

use super::{Extracted, read_html, read_pdf};
use crate::Error;
use crate::backend::{Cancel, Result};
use crate::net::{Http, Request, Response, Transport, encode_component, percent_decode};
use crate::youtube::{Json3, captions_to_text, parse_watch_page, pick_track, youtube_id};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15";

const TIMEOUT: Duration = Duration::from_secs(120);

fn get(http: &dyn Http, req: Request) -> Result<Response> {
    http.send(req.timeout(TIMEOUT)).map_err(|e| match e {
        Transport::Timeout => Error::Network("The page took too long to load".into()),
        Transport::Failed(m) => Error::Network(m),
    })
}

fn reason(status: u16) -> &'static str {
    ureq::http::StatusCode::from_u16(status).ok().and_then(|s| s.canonical_reason()).unwrap_or("")
}

/// The last path part of a URL, percent-decoded, as a title.
fn last_segment(url: &str) -> String {
    let last = url.rsplit('/').next().unwrap_or(url);
    percent_decode(last).unwrap_or_else(|| last.to_string())
}

/// Reads a web page, a PDF link or a YouTube video's captions.
/// `languages` are the user's preferred caption languages, e.g. `["de-DE", "en"]`.
pub fn extract_url(http: &dyn Http, url: &str, languages: &[String], cancel: &Cancel) -> Result<Extracted> {
    cancel.check()?;
    if let Some(id) = youtube_id(url) {
        return read_youtube(http, &id, languages, cancel);
    }
    let response = get(
        http,
        Request::get(url).header("User-Agent", USER_AGENT).header("Accept", "text/html,application/xhtml+xml,application/pdf;q=0.9,*/*;q=0.8"),
    )?;
    if !response.ok() {
        let status = response.status;
        let line = format!("{status} {}", reason(status));
        return Err(Error::other(format!("Could not download the page ({})", line.trim())));
    }
    let ty = response.header("content-type").unwrap_or_default().to_ascii_lowercase();
    let lower = url.to_ascii_lowercase();
    let is_pdf_url = lower.ends_with(".pdf") || lower.contains(".pdf?");
    let body = response.bytes().map_err(|e| Error::Network(e.to_string()))?;
    cancel.check()?;
    if ty.contains("application/pdf") || is_pdf_url {
        let pdf = read_pdf(&body)?;
        let title = if pdf.title.is_empty() { last_segment(url) } else { pdf.title };
        return Ok(Extracted { title, text: pdf.text, media: false });
    }
    let text = String::from_utf8_lossy(&body).into_owned();
    if ty.starts_with("text/plain") || ty.contains("markdown") {
        return Ok(Extracted { title: last_segment(url), text: text.trim().to_string(), media: false });
    }
    let page = read_html(&text, Some(url));
    Ok(Extracted { title: page.title, text: page.text, media: false })
}

/// Captions of a YouTube video, in the user's languages when available.
pub fn read_youtube(http: &dyn Http, id: &str, languages: &[String], cancel: &Cancel) -> Result<Extracted> {
    let mut langs: Vec<String> = Vec::new();
    for l in languages.iter().map(String::as_str).chain(["en"]) {
        let base = l.split('-').next().unwrap_or(l).to_string();
        if !langs.contains(&base) {
            langs.push(base);
        }
    }
    let page = get(
        http,
        Request::get(format!("https://www.youtube.com/watch?v={}&hl=en", encode_component(id)))
            .header("User-Agent", USER_AGENT)
            .header("Accept-Language", langs.join(",")),
    )?;
    if !page.ok() {
        return Err(Error::other(format!("YouTube did not answer ({})", page.status)));
    }
    let html = page.text().map_err(|e| Error::Network(e.to_string()))?;
    cancel.check()?;
    let watch = parse_watch_page(&html)?;
    let track = pick_track(&watch.tracks, &langs).ok_or_else(|| Error::other("This video has no captions to read."))?;
    let captions = get(http, Request::get(format!("{}&fmt=json3", track.base_url)).header("User-Agent", USER_AGENT))?;
    if !captions.ok() {
        return Err(Error::other(format!("Could not download the captions ({})", captions.status)));
    }
    let json3: Json3 = serde_json::from_slice(&captions.bytes().map_err(|e| Error::Network(e.to_string()))?)?;
    let text = captions_to_text(&json3);
    if text.is_empty() {
        return Err(Error::other("The captions for this video are empty."));
    }
    Ok(Extracted { title: watch.title, text, media: false })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Mutex;

    #[test]
    fn youtube_links_read_captions_in_the_preferred_language() {
        let player = json!({ "videoDetails": { "title": "Talk" }, "captions": { "playerCaptionsTracklistRenderer": { "captionTracks": [
            { "baseUrl": "https://yt/api/timedtext?v=x&lang=en", "languageCode": "en" },
            { "baseUrl": "https://yt/api/timedtext?v=x&lang=de", "languageCode": "de" },
        ] } } });
        let seen = Mutex::new(Vec::new());
        let http = |r: Request| {
            seen.lock().unwrap().push((r.url.clone(), r.header_value("accept-language").map(str::to_string)));
            if r.url.starts_with("https://www.youtube.com/watch") {
                Ok(Response::new(200, format!("var ytInitialPlayerResponse = {player};")))
            } else {
                Ok(Response::new(200, json!({ "events": [{ "tStartMs": 0, "dDurationMs": 10, "segs": [{ "utf8": "Hallo" }] }] }).to_string()))
            }
        };
        let out = extract_url(&http, "https://youtu.be/abcdefgh", &["de-DE".into(), "de".into()], &Cancel::new()).unwrap();
        assert_eq!(out, Extracted { title: "Talk".into(), text: "Hallo".into(), media: false });
        let seen = seen.lock().unwrap();
        assert_eq!(seen[0], ("https://www.youtube.com/watch?v=abcdefgh&hl=en".into(), Some("de,en".into())));
        assert_eq!(seen[1].0, "https://yt/api/timedtext?v=x&lang=de&fmt=json3");
    }

    #[test]
    fn videos_without_captions_say_so() {
        let http = |_: Request| Ok(Response::new(200, "ytInitialPlayerResponse = {\"videoDetails\":{}}"));
        let err = extract_url(&http, "https://www.youtube.com/watch?v=abcdefgh", &[], &Cancel::new()).unwrap_err();
        assert_eq!(err.to_string(), "This video has no captions to read.");
    }

    #[test]
    fn pages_plain_text_and_pdfs_are_read_by_type() {
        let http = |r: Request| {
            assert_eq!(r.header_value("user-agent"), Some(USER_AGENT));
            Ok(match r.url.as_str() {
                "https://x/a%20file.txt" => Response::new(200, " text body \n").with_header("Content-Type", "text/plain; charset=utf-8"),
                "https://x/doc" => Response::new(200, super::super::pdf::tests::sample_pdf()).with_header("Content-Type", "application/pdf"),
                "https://x/page" => Response::new(200, "<html><head><title>Page</title></head><body><p>Short page</p></body></html>").with_header("Content-Type", "text/html"),
                _ => Response::new(404, ""),
            })
        };
        let c = Cancel::new();
        assert_eq!(extract_url(&http, "https://x/a%20file.txt", &[], &c).unwrap(), Extracted { title: "a file.txt".into(), text: "text body".into(), media: false });
        assert_eq!(extract_url(&http, "https://x/doc", &[], &c).unwrap().title, "A Test Paper");
        let page = extract_url(&http, "https://x/page", &[], &c).unwrap();
        assert_eq!(page.title, "Page");
        assert!(page.text.contains("Short page"));
        assert_eq!(extract_url(&http, "https://x/missing", &[], &c).unwrap_err().to_string(), "Could not download the page (404 Not Found)");
    }
}

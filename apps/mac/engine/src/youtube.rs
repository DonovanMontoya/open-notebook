//! YouTube captions, read from the watch page the way the server's
//! youtube-transcript-api does: find the caption tracks, prefer the user's
//! languages, and turn the timed text into paragraphs. Port of
//! `packages/engine/src/youtube.ts`; the download itself is `extract::read_youtube`.

use serde::Deserialize;
use serde_json::Value;

use crate::Error;
use crate::backend::Result;

/// The video id in a watch, youtu.be, shorts, embed or live URL.
pub fn youtube_id(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    let path = u.path();
    if host == "youtu.be" {
        let id = &path[1.min(path.len())..];
        return (!id.is_empty()).then(|| id.to_string());
    }
    if host == "youtube.com" || host.ends_with(".youtube.com") {
        if path == "/watch" {
            return u.query_pairs().find(|(k, _)| k == "v").map(|(_, v)| v.into_owned());
        }
        for prefix in ["/shorts/", "/embed/", "/live/"] {
            if let Some(rest) = path.strip_prefix(prefix) {
                let id: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
                if id.len() >= 6 {
                    return Some(id);
                }
            }
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptionTrack {
    pub base_url: String,
    pub language_code: String,
    pub name: Option<String>,
    /// "asr" for automatic captions.
    pub kind: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchPage {
    pub title: String,
    pub tracks: Vec<CaptionTrack>,
}

/// Pulls ytInitialPlayerResponse out of a watch page.
pub fn parse_watch_page(html: &str) -> Result<WatchPage> {
    let unreadable = || Error::other("This YouTube page could not be read.");
    let marker = html.find("ytInitialPlayerResponse").ok_or_else(unreadable)?;
    let start = marker + html[marker..].find('{').ok_or_else(unreadable)?;
    let bytes = html.as_bytes();
    let (mut depth, mut end, mut in_string) = (0usize, start, false);
    while end < bytes.len() {
        let c = bytes[end];
        if in_string {
            if c == b'\\' {
                end += 1;
            } else if c == b'"' {
                in_string = false;
            }
        } else if c == b'"' {
            in_string = true;
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                break;
            }
        }
        end += 1;
    }
    let json = html.get(start..(end + 1).min(html.len())).ok_or_else(unreadable)?;
    let player: Value = serde_json::from_str(json).map_err(|_| unreadable())?;
    let tracks = player
        .pointer("/captions/playerCaptionsTracklistRenderer/captionTracks")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|t| {
                    Some(CaptionTrack {
                        base_url: t.get("baseUrl")?.as_str()?.to_string(),
                        language_code: t.get("languageCode").and_then(Value::as_str).unwrap_or_default().to_string(),
                        name: t.pointer("/name/simpleText").and_then(Value::as_str).map(str::to_string),
                        kind: t.get("kind").and_then(Value::as_str).map(str::to_string),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let title = player.pointer("/videoDetails/title").and_then(Value::as_str).unwrap_or("YouTube video").to_string();
    Ok(WatchPage { title, tracks })
}

/// Manual captions in a preferred language first, then automatic ones, then anything.
pub fn pick_track<'a>(tracks: &'a [CaptionTrack], languages: &[String]) -> Option<&'a CaptionTrack> {
    let lang = |t: &CaptionTrack| t.language_code.to_lowercase().split('-').next().unwrap_or_default().to_string();
    let default = ["en".to_string()];
    let languages = if languages.is_empty() { &default[..] } else { languages };
    for want in languages.iter().map(|l| l.to_lowercase()) {
        if let Some(t) = tracks.iter().find(|t| lang(t) == want && t.kind.as_deref() != Some("asr")) {
            return Some(t);
        }
        if let Some(t) = tracks.iter().find(|t| lang(t) == want) {
            return Some(t);
        }
    }
    tracks.iter().find(|t| t.kind.as_deref() != Some("asr")).or(tracks.first())
}

#[derive(Debug, Default, Deserialize)]
pub struct Json3 {
    #[serde(default)]
    pub events: Option<Vec<Json3Event>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Json3Event {
    #[serde(default)]
    pub t_start_ms: Option<f64>,
    #[serde(default)]
    pub d_duration_ms: Option<f64>,
    #[serde(default)]
    pub segs: Option<Vec<Json3Seg>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Json3Seg {
    #[serde(default)]
    pub utf8: Option<String>,
}

/// YouTube's json3 timed text as readable paragraphs, breaking on long pauses.
pub fn captions_to_text(json3: &Json3) -> String {
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut last_end = 0.0;
    for e in json3.events.iter().flatten() {
        let joined: String = e.segs.iter().flatten().map(|s| s.utf8.as_deref().unwrap_or_default()).collect();
        let text = joined.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        let start = e.t_start_ms.unwrap_or(last_end);
        if !current.is_empty() && (start - last_end > 2500.0 || current.chars().count() > 600) {
            paragraphs.push(current.trim().to_string());
            current.clear();
        }
        current.push_str(&text);
        current.push(' ');
        last_end = start + e.d_duration_ms.unwrap_or(0.0);
    }
    if !current.trim().is_empty() {
        paragraphs.push(current.trim().to_string());
    }
    paragraphs.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn youtube_id_understands_the_common_url_shapes() {
        assert_eq!(youtube_id("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=10").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(youtube_id("https://youtu.be/dQw4w9WgXcQ").as_deref(), Some("dQw4w9WgXcQ"));
        assert_eq!(youtube_id("https://www.youtube.com/shorts/abcdef123").as_deref(), Some("abcdef123"));
        assert_eq!(youtube_id("https://m.youtube.com/embed/abc_de-f?x=1").as_deref(), Some("abc_de-f"));
        assert_eq!(youtube_id("https://www.youtube.com/live/short"), None);
        assert_eq!(youtube_id("https://example.com/watch?v=x"), None);
        assert_eq!(youtube_id("https://notyoutube.com/watch?v=x"), None);
        assert_eq!(youtube_id("not a url"), None);
    }

    #[test]
    fn parse_watch_page_finds_the_title_and_caption_tracks_even_with_braces_inside_strings() {
        let player = json!({
            "videoDetails": { "title": "A talk {with braces} and \"quotes\"" },
            "captions": { "playerCaptionsTracklistRenderer": { "captionTracks": [
                { "baseUrl": "https://yt/auto", "languageCode": "en", "kind": "asr" },
                { "baseUrl": "https://yt/es", "languageCode": "es" },
                { "baseUrl": "https://yt/en", "languageCode": "en-GB", "name": { "simpleText": "English" } },
            ] } },
        });
        let html = format!("<script>var ytInitialPlayerResponse = {player};var other = {{}};</script>");
        let page = parse_watch_page(&html).unwrap();
        assert_eq!(page.title, "A talk {with braces} and \"quotes\"");
        assert_eq!(page.tracks.len(), 3);
        assert_eq!(page.tracks[2].name.as_deref(), Some("English"));
        let langs = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(pick_track(&page.tracks, &langs(&["en"])).unwrap().base_url, "https://yt/en", "manual captions beat automatic ones");
        assert_eq!(pick_track(&page.tracks, &langs(&["es"])).unwrap().base_url, "https://yt/es");
        assert_eq!(pick_track(&page.tracks, &langs(&["fr"])).unwrap().base_url, "https://yt/es", "falls back to any manual track");
        assert_eq!(pick_track(&[], &langs(&["en"])), None);
    }

    #[test]
    fn a_page_without_a_player_is_an_error() {
        assert_eq!(parse_watch_page("<html></html>").unwrap_err().to_string(), "This YouTube page could not be read.");
        let page = parse_watch_page("ytInitialPlayerResponse = {}").unwrap();
        assert_eq!((page.title.as_str(), page.tracks.len()), ("YouTube video", 0));
    }

    #[test]
    fn captions_to_text_joins_segments_and_starts_paragraphs_at_long_pauses() {
        let json3: Json3 = serde_json::from_value(json!({ "events": [
            { "tStartMs": 0, "dDurationMs": 1000, "segs": [{ "utf8": "Hello" }, { "utf8": " there." }] },
            { "tStartMs": 1100, "dDurationMs": 900, "segs": [{ "utf8": "Second line." }] },
            { "tStartMs": 9000, "dDurationMs": 1000, "segs": [{ "utf8": "After a pause." }] },
            { "tStartMs": 10000, "segs": [{ "utf8": "\n" }] },
            { "tStartMs": 10500 },
        ] }))
        .unwrap();
        assert_eq!(captions_to_text(&json3), "Hello there. Second line.\n\nAfter a pause.");
    }
}

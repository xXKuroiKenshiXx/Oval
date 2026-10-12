//! Lists every video of a channel or playlist without downloading anything,
//! streaming entries to the UI page by page so the grid fills in right away.
//!
//! The fast "flat" listing has no thumbnails on many sites (PeerTube, Vimeo,
//! ...), so a second, slower pass fills them in afterwards.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};

use crate::downloads;
use crate::engine::{self, Line};

#[derive(Clone, Serialize)]
pub struct Entry {
    pub id: String,
    pub url: String,
    pub title: String,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Batch {
    token: u64,
    channel: Option<String>,
    entries: Vec<Entry>,
}

/// Thumbnail and duration found by the second pass, by position in the list.
#[derive(Clone, Serialize)]
struct Detail {
    index: usize,
    thumbnail: Option<String>,
    duration: Option<f64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Details {
    token: u64,
    items: Vec<Detail>,
}

#[derive(Serialize)]
pub struct Summary {
    pub token: u64,
    pub channel: Option<String>,
    pub count: usize,
}

/// The listing currently running: (generation, yt-dlp handle).
static CURRENT: Mutex<(u64, Option<engine::Handle>)> = Mutex::new((0, None));

pub fn cancel(app: &AppHandle) {
    let handle = {
        let mut cur = CURRENT.lock().unwrap();
        cur.0 += 1;
        cur.1.take()
    };
    if let Some(handle) = handle {
        engine::kill(app, handle);
    }
}

/// `https://youtube.com/@name` lists the channel's tabs, not its videos, so the
/// chosen tab is appended (or swapped in) for YouTube channel URLs.
pub fn normalize(url: &str, tab: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if let Some((host, name)) = streaming_channel(url) {
        return match host {
            "kick.com" => format!(
                "https://kick.com/{name}/{}",
                if tab == "clips" { "clips" } else { "videos" }
            ),
            _ if tab == "clips" => {
                format!("https://www.twitch.tv/{name}/clips?filter=clips&range=all")
            }
            _ => format!("https://www.twitch.tv/{name}/videos?filter=all&sort=time"),
        };
    }
    let lower = url.to_ascii_lowercase();
    let is_channel = lower.contains("youtube.com/")
        && !lower.contains("list=")
        && ["/@", "/channel/", "/c/", "/user/"]
            .iter()
            .any(|p| lower.contains(p));
    if !is_channel {
        return url.to_string();
    }
    let tabs = [
        "/videos",
        "/shorts",
        "/streams",
        "/featured",
        "/playlists",
        "/live",
        "/community",
    ];
    let mut base = url.split(['?', '#']).next().unwrap_or(url).to_string();
    if let Some(t) = tabs
        .iter()
        .find(|t| base.to_ascii_lowercase().ends_with(*t))
    {
        base.truncate(base.len() - t.len());
    }
    let tab = match tab {
        "shorts" => "shorts",
        "streams" => "streams",
        _ => "videos",
    };
    format!("{base}/{tab}")
}

/// Accept channel pages, but never an individual video or clip URL.
fn streaming_channel(url: &str) -> Option<(&'static str, &str)> {
    let (_, rest) = url.split_once("://")?;
    let (host, path) = rest.split_once('/')?;
    let host = host.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .unwrap_or(&host);
    let site = match host {
        "kick.com" => "kick.com",
        "twitch.tv" => "twitch.tv",
        _ => return None,
    };
    if url
        .split_once('?')
        .is_some_and(|(_, query)| query.split('&').any(|p| p.starts_with("clip=")))
    {
        return None;
    }
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let mut pieces = path.split('/');
    let name = pieces.next()?;
    if name.is_empty()
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
        || matches!(
            name,
            "videos" | "clips" | "categories" | "directory" | "search" | "auth" | "collections"
        )
    {
        return None;
    }
    let section = pieces.next();
    if pieces.next().is_some()
        || section.is_some_and(|s| !matches!(s, "videos" | "clips" | "profile"))
    {
        return None;
    }
    Some((site, name))
}

/// `token` comes from the UI and is echoed in every batch so it can drop
/// entries that belong to an older listing.
pub async fn list(app: &AppHandle, url: &str, tab: &str, token: u64) -> Result<Summary, String> {
    cancel(app);
    let listing = CURRENT.lock().unwrap().0;
    let url = normalize(url, tab);

    let args = [
        "--ignore-config",
        "--no-warnings",
        "--flat-playlist",
        "--lazy-playlist",
        "--print",
        "%(.{id,url,webpage_url,title,duration,thumbnail,thumbnails,playlist_uploader,channel,uploader,playlist_title})j",
        "--",
        &url,
    ]
    .map(String::from)
    .to_vec();
    let mut task = engine::ytdlp(app, args).await?;
    {
        let mut cur = CURRENT.lock().unwrap();
        if cur.0 != listing {
            engine::kill(app, task.handle);
            return Err("cancelled".into());
        }
        cur.1 = Some(task.handle);
    }

    let youtube = url.contains("youtube.com") || url.contains("youtu.be");
    let mut channel = streaming_channel(&url).map(|(_, name)| name.to_string());
    let mut pending = Vec::new();
    let mut count = 0usize;
    let mut missing = 0usize;
    let mut error = None;
    let mut last_emit = Instant::now();
    while let Some(line) = task.next_line().await {
        let raw = match line {
            Line::Out(raw) => raw,
            Line::Err(err) => {
                if let Some(msg) = err.strip_prefix("ERROR:") {
                    error = Some(downloads::clean_error(msg));
                }
                continue;
            }
        };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(entry) = parse_entry(&v, youtube) else {
            continue;
        };
        if channel.is_none() {
            channel = ["playlist_uploader", "channel", "uploader", "playlist_title"]
                .iter()
                .find_map(|k| v.get(*k).and_then(Value::as_str))
                .map(str::to_string);
        }
        missing += usize::from(entry.thumbnail.is_none());
        pending.push(entry);
        count += 1;
        if pending.len() >= 60 || last_emit.elapsed() > Duration::from_millis(150) {
            if CURRENT.lock().unwrap().0 != listing {
                break;
            }
            let _ = app.emit(
                "channel-batch",
                Batch {
                    token,
                    channel: channel.clone(),
                    entries: std::mem::take(&mut pending),
                },
            );
            last_emit = Instant::now();
        }
    }
    let success = task.success();
    {
        let mut cur = CURRENT.lock().unwrap();
        if cur.0 != listing {
            return Err("cancelled".into());
        }
        cur.1 = None;
    }
    if !pending.is_empty() {
        let _ = app.emit(
            "channel-batch",
            Batch {
                token,
                channel: channel.clone(),
                entries: pending,
            },
        );
    }
    if count == 0 && !success {
        return Err(error.unwrap_or_else(|| "No se encontraron vídeos".into()));
    }
    if missing > 0 {
        tauri::async_runtime::spawn(fill_details(app.clone(), url, token, listing));
    }
    Ok(Summary {
        token,
        channel,
        count,
    })
}

/// Second pass: full extraction of each entry, streamed, only for thumbnails
/// and durations. Runs until done or until another listing starts.
async fn fill_details(app: AppHandle, url: String, token: u64, listing: u64) {
    let args = [
        "--ignore-config",
        "--no-warnings",
        "--ignore-errors",
        "--lazy-playlist",
        "--skip-download",
        "--print",
        "%(.{playlist_index,thumbnail,duration})j",
        "--",
        &url,
    ]
    .map(String::from)
    .to_vec();
    let Ok(mut task) = engine::ytdlp(&app, args).await else {
        return;
    };
    {
        let mut cur = CURRENT.lock().unwrap();
        if cur.0 != listing {
            engine::kill(&app, task.handle);
            return;
        }
        cur.1 = Some(task.handle); // so `cancel` stops this pass too
    }
    let mut pending = Vec::new();
    let mut last_emit = Instant::now();
    while let Some(line) = task.next_line().await {
        if CURRENT.lock().unwrap().0 != listing {
            engine::kill(&app, task.handle);
            return;
        }
        let Line::Out(raw) = line else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let Some(index) = v.get("playlist_index").and_then(Value::as_u64) else {
            continue;
        };
        pending.push(Detail {
            index: (index as usize).saturating_sub(1),
            thumbnail: v
                .get("thumbnail")
                .and_then(Value::as_str)
                .filter(|t| t.starts_with("http"))
                .map(str::to_string),
            duration: v.get("duration").and_then(Value::as_f64),
        });
        if pending.len() >= 12 || last_emit.elapsed() > Duration::from_millis(200) {
            let _ = app.emit(
                "channel-details",
                Details {
                    token,
                    items: std::mem::take(&mut pending),
                },
            );
            last_emit = Instant::now();
        }
    }
    if !pending.is_empty() {
        let _ = app.emit(
            "channel-details",
            Details {
                token,
                items: pending,
            },
        );
    }
    let mut cur = CURRENT.lock().unwrap();
    if cur.0 == listing {
        cur.1 = None;
    }
}

fn parse_entry(v: &Value, youtube: bool) -> Option<Entry> {
    let s = |k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty() && *s != "NA")
    };
    let id = s("id")?.to_string();
    let url = s("url").or_else(|| s("webpage_url"))?.to_string();
    let thumbnail = if youtube && id.len() == 11 {
        Some(format!("https://i.ytimg.com/vi/{id}/mqdefault.jpg"))
    } else {
        s("thumbnail").map(str::to_string).or_else(|| {
            v.get("thumbnails")?
                .as_array()?
                .iter()
                .rev()
                .find_map(|t| t.get("url")?.as_str().map(str::to_string))
        })
    };
    Some(Entry {
        title: s("title").unwrap_or(&id).to_string(),
        duration: v.get("duration").and_then(Value::as_f64),
        id,
        url,
        thumbnail,
    })
}

#[cfg(test)]
mod tests {
    use super::{normalize, parse_entry, streaming_channel};

    #[test]
    fn channel_urls_get_a_tab() {
        assert_eq!(
            normalize("https://www.youtube.com/@abc", "videos"),
            "https://www.youtube.com/@abc/videos"
        );
        assert_eq!(
            normalize("https://youtube.com/@abc/featured/", "shorts"),
            "https://youtube.com/@abc/shorts"
        );
        assert_eq!(
            normalize("https://www.youtube.com/playlist?list=PL1", "videos"),
            "https://www.youtube.com/playlist?list=PL1"
        );
    }

    #[test]
    fn streaming_channels_list_archives_and_clips() {
        assert_eq!(
            normalize("https://kick.com/lacobraaa/videos", "videos"),
            "https://kick.com/lacobraaa/videos"
        );
        assert_eq!(
            normalize("https://kick.com/lacobraaa/videos", "clips"),
            "https://kick.com/lacobraaa/clips"
        );
        assert_eq!(
            normalize("https://www.kick.com/lacobraaa", "videos"),
            "https://kick.com/lacobraaa/videos"
        );
        assert_eq!(
            normalize("https://www.twitch.tv/ibai", "videos"),
            "https://www.twitch.tv/ibai/videos?filter=all&sort=time"
        );
        assert_eq!(
            normalize("https://m.twitch.tv/ibai/videos?filter=all", "clips"),
            "https://www.twitch.tv/ibai/clips?filter=clips&range=all"
        );
        assert!(streaming_channel(
            "https://kick.com/lacobraaa/videos/1114e3e9-17a3-4308-9c26-990e1413ae17"
        )
        .is_none());
        assert!(streaming_channel("https://kick.com/lacobraaa?clip=clip_123").is_none());
        assert!(streaming_channel("https://kick.com.evil/lacobraaa/videos").is_none());
        assert!(streaming_channel("https://www.twitch.tv/videos/12345").is_none());
    }

    #[test]
    fn flat_streaming_entries_keep_download_links_and_thumbnails() {
        let raw = serde_json::json!({
            "id": "clip_123", "url": "https://kick.com/lacobraaa/clips/clip_123",
            "title": "Clip", "duration": 17, "thumbnail": "https://clips.kick.com/thumbnail.webp"
        });
        let entry = parse_entry(&raw, false).unwrap();
        assert_eq!(entry.id, "clip_123");
        assert_eq!(entry.url, raw["url"].as_str().unwrap());
        assert_eq!(entry.duration, Some(17.0));
        assert_eq!(entry.thumbnail.as_deref(), raw["thumbnail"].as_str());
    }
}

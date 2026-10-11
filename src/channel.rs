//! Lists every video of a channel or playlist without downloading anything,
//! streaming entries to the UI page by page so the grid fills in right away.
//!
//! The fast "flat" listing has no thumbnails on many sites (PeerTube, Vimeo,
//! ...), so a second, slower pass fills them in afterwards.

use std::process::Stdio;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::{downloads, platform, tools};

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

/// The listing currently running: (generation, pid).
static CURRENT: Mutex<(u64, Option<u32>)> = Mutex::new((0, None));

pub fn cancel() {
    let pid = {
        let mut cur = CURRENT.lock().unwrap();
        cur.0 += 1;
        cur.1.take()
    };
    if let Some(pid) = pid {
        platform::kill_tree(pid);
    }
}

/// `https://youtube.com/@name` lists the channel's tabs, not its videos, so the
/// chosen tab is appended (or swapped in) for YouTube channel URLs.
pub fn normalize(url: &str, tab: &str) -> String {
    let url = url.trim().trim_end_matches('/');
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

/// `token` comes from the UI and is echoed in every batch so it can drop
/// entries that belong to an older listing.
pub async fn list(app: &AppHandle, url: &str, tab: &str, token: u64) -> Result<Summary, String> {
    cancel();
    let listing = CURRENT.lock().unwrap().0;
    let tools = tools::ensure(app).await?;
    let url = normalize(url, tab);

    let mut cmd = Command::new(&tools.ytdlp);
    cmd.args([
        "--ignore-config",
        "--no-warnings",
        "--flat-playlist",
        "--lazy-playlist",
        "--print",
        "%(.{id,url,webpage_url,title,duration,thumbnail,thumbnails,playlist_uploader,channel,uploader,playlist_title})j",
        "--",
        &url,
    ])
    .env("PYTHONIOENCODING", "utf-8")
    .env("PYTHONUTF8", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);
    platform::prepare(&mut cmd);

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    platform::adopt_child(&child);
    {
        let mut cur = CURRENT.lock().unwrap();
        if cur.0 != listing {
            let _ = child.start_kill();
            return Err("cancelled".into());
        }
        cur.1 = child.id();
    }

    let stderr = child.stderr.take().expect("piped stderr");
    let err_task = tokio::spawn(async move {
        let mut last = None;
        let mut lines = BufReader::new(stderr).split(b'\n');
        while let Ok(Some(raw)) = lines.next_segment().await {
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if let Some(msg) = line.strip_prefix("ERROR:") {
                last = Some(downloads::clean_error(msg));
            }
        }
        last
    });

    let youtube = url.contains("youtube.com") || url.contains("youtu.be");
    let mut channel: Option<String> = None;
    let mut pending = Vec::new();
    let mut count = 0usize;
    let mut missing = 0usize;
    let mut last_emit = Instant::now();
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).split(b'\n');
    while let Ok(Some(raw)) = lines.next_segment().await {
        let Ok(v) = serde_json::from_slice::<Value>(&raw) else {
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
    let status = child.wait().await.ok();
    let error = err_task.await.ok().flatten();
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
    if count == 0 && !status.is_some_and(|s| s.success()) {
        return Err(error.unwrap_or_else(|| "No se encontraron vídeos".into()));
    }
    if missing > 0 {
        tauri::async_runtime::spawn(fill_details(
            app.clone(),
            tools.ytdlp.clone(),
            url,
            token,
            listing,
        ));
    }
    Ok(Summary {
        token,
        channel,
        count,
    })
}

/// Second pass: full extraction of each entry, streamed, only for thumbnails
/// and durations. Runs until done or until another listing starts.
async fn fill_details(
    app: AppHandle,
    ytdlp: std::path::PathBuf,
    url: String,
    token: u64,
    listing: u64,
) {
    let mut cmd = Command::new(ytdlp);
    cmd.args([
        "--ignore-config",
        "--no-warnings",
        "--ignore-errors",
        "--lazy-playlist",
        "--skip-download",
        "--print",
        "%(.{playlist_index,thumbnail,duration})j",
        "--",
        &url,
    ])
    .env("PYTHONIOENCODING", "utf-8")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .kill_on_drop(true);
    platform::prepare(&mut cmd);
    let Ok(mut child) = cmd.spawn() else { return };
    platform::adopt_child(&child);
    {
        let mut cur = CURRENT.lock().unwrap();
        if cur.0 != listing {
            return;
        }
        cur.1 = child.id(); // so `cancel` stops this pass too
    }
    let mut pending = Vec::new();
    let mut last_emit = Instant::now();
    let mut lines = BufReader::new(child.stdout.take().expect("piped stdout")).split(b'\n');
    while let Ok(Some(raw)) = lines.next_segment().await {
        if CURRENT.lock().unwrap().0 != listing {
            return;
        }
        let Ok(v) = serde_json::from_slice::<Value>(&raw) else {
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
    let _ = child.wait().await;
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
    use super::normalize;

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
}

//! What the browser extension needs to cut a live stream: whether a Twitch or
//! Kick channel is live, when the broadcast started and the link of the
//! recording both sites keep while it runs. A part of the stream is then a
//! normal section of that recording, seconds from the start.

use serde::Serialize;
use serde_json::{json, Value};
use tauri::AppHandle;

use crate::extras::{client, dump_json};

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct LiveInfo {
    pub live: bool,
    /// Unix seconds; second 0 of the recording.
    pub started_at: Option<f64>,
    /// The recording that grows with the stream; `None` if the channel
    /// doesn't keep one (then only recording from now on is possible).
    pub vod: Option<String>,
    pub title: Option<String>,
}

/// Public client ID of Twitch's own website, the one yt-dlp uses too.
const TWITCH_CLIENT_ID: &str = "kimne78kx3ncx6brgo4mv6wki5h1ko";

pub async fn info(app: &AppHandle, url: &str) -> Result<LiveInfo, String> {
    if let Some(login) = channel(url, &["twitch.tv"], TWITCH_PAGES) {
        return twitch(&login).await;
    }
    if let Some(slug) = channel(url, &["kick.com"], KICK_PAGES) {
        return kick(app, &slug).await;
    }
    Ok(LiveInfo::default())
}

const TWITCH_PAGES: &[&str] = &[
    "directory",
    "videos",
    "settings",
    "search",
    "downloads",
    "jobs",
    "p",
    "turbo",
    "drops",
    "wallet",
    "inventory",
    "subscriptions",
    "friends",
    "messages",
    "moderator",
    "u",
    "login",
    "signup",
    "prime",
];
const KICK_PAGES: &[&str] = &[
    "categories",
    "search",
    "browse",
    "following",
    "auth",
    "video",
    "videos",
];

/// "name" for https://www.twitch.tv/name or https://kick.com/name (a channel
/// page, not a video, clip or site section).
fn channel(url: &str, hosts: &[&str], reserved: &[&str]) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.to_ascii_lowercase();
    let host = host
        .strip_prefix("www.")
        .or_else(|| host.strip_prefix("m."))
        .unwrap_or(&host);
    if !hosts.contains(&host) {
        return None;
    }
    let path = path.split(['?', '#']).next()?.trim_end_matches('/');
    let valid = !path.is_empty()
        && !path.contains('/')
        && path
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !reserved.contains(&path.to_ascii_lowercase().as_str());
    valid.then(|| path.to_ascii_lowercase())
}

async fn twitch(login: &str) -> Result<LiveInfo, String> {
    let query = json!({
        "query": "query($login: String!) { user(login: $login) { stream { createdAt title archiveVideo { id } } } }",
        "variables": { "login": login },
    });
    let data: Value = client()
        .post("https://gql.twitch.tv/gql")
        .header("Client-Id", TWITCH_CLIENT_ID)
        .json(&query)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("Twitch no respondió: {e}"))?
        .json()
        .await
        .map_err(|e| format!("Respuesta inesperada de Twitch: {e}"))?;
    let stream = &data["data"]["user"]["stream"];
    if !stream.is_object() {
        return Ok(LiveInfo::default());
    }
    Ok(LiveInfo {
        live: true,
        started_at: stream["createdAt"].as_str().and_then(unix_time),
        vod: stream["archiveVideo"]["id"]
            .as_str()
            .map(|id| format!("https://www.twitch.tv/videos/{id}")),
        title: stream["title"].as_str().map(String::from),
    })
}

/// Kick's API turns away plain HTTP clients, so this goes through yt-dlp
/// (which looks like a browser) and Oval's `oval:kick-live:` extractor.
async fn kick(app: &AppHandle, slug: &str) -> Result<LiveInfo, String> {
    let extra = vec!["--ignore-no-formats-error".to_string()];
    let data: Value = dump_json(app, extra, &format!("oval:kick-live:{slug}")).await?;
    if data["live_status"].as_str() != Some("is_live") {
        return Ok(LiveInfo::default());
    }
    Ok(LiveInfo {
        live: true,
        started_at: data["release_timestamp"].as_f64(),
        vod: data["vod_url"].as_str().map(String::from),
        title: data["title"].as_str().map(String::from),
    })
}

/// "2026-10-16T14:38:09Z" (Twitch) or "2026-10-16 15:19:27" (Kick, UTC).
fn unix_time(s: &str) -> Option<f64> {
    let n: Vec<i64> = s
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .take(6)
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let [y, m, d, hh, mm, ss] = n[..] else {
        return None;
    };
    // Days from 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * ((m + 9) % 12) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some((days * 86_400 + hh * 3600 + mm * 60 + ss) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_pages_are_recognized() {
        let twitch = |u| channel(u, &["twitch.tv"], TWITCH_PAGES);
        assert_eq!(
            twitch("https://www.twitch.tv/Caedrel?sr=a").as_deref(),
            Some("caedrel")
        );
        assert_eq!(
            twitch("https://m.twitch.tv/caedrel/").as_deref(),
            Some("caedrel")
        );
        assert_eq!(twitch("https://www.twitch.tv/videos/123"), None);
        assert_eq!(twitch("https://www.twitch.tv/directory"), None);
        assert_eq!(twitch("https://www.twitch.tv/caedrel/clip/abc"), None);
        assert_eq!(twitch("https://twitch.tv.evil.com/caedrel"), None);
        let kick = |u| channel(u, &["kick.com"], KICK_PAGES);
        assert_eq!(kick("https://kick.com/maherco").as_deref(), Some("maherco"));
        assert_eq!(kick("https://kick.com/maherco/videos/uuid"), None);
        assert_eq!(kick("https://kick.com/categories"), None);
    }

    #[test]
    fn utc_times() {
        assert_eq!(unix_time("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(unix_time("2026-10-16T14:38:09Z"), Some(1_792_161_489.0));
        assert_eq!(unix_time("2026-10-16 15:19:27"), Some(1_792_163_967.0));
        assert_eq!(unix_time("nope"), None);
    }
}

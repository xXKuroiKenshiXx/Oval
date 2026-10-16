//! Everything the browser extension asks for besides downloads: the video's
//! details, its transcript and its thumbnail, saved next to the downloads.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::Value;
use tauri::AppHandle;

use crate::downloads::{clean_error, sanitize, youtube_args, Section};
use crate::engine::{self, Line};
use crate::transcript::{self, Format, Meta};

/// The parts of yt-dlp's JSON that are used here.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Info {
    pub id: String,
    pub title: String,
    pub channel: Option<String>,
    pub uploader: Option<String>,
    pub upload_date: Option<String>,
    pub webpage_url: Option<String>,
    pub language: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub thumbnails: Vec<Thumb>,
    pub subtitles: std::collections::BTreeMap<String, Vec<Track>>,
    pub automatic_captions: std::collections::BTreeMap<String, Vec<Track>>,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Thumb {
    pub url: String,
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Track {
    pub ext: String,
    pub url: String,
}

impl Info {
    /// "Title [id]", the same stem Oval gives to the video itself.
    fn stem(&self, section: Option<Section>) -> String {
        let mut stem = format!("{} [{}]", sanitize(&self.title), sanitize(&self.id));
        if let Some(s) = section {
            stem.push(' ');
            stem.push_str(&s.label());
        }
        stem
    }
}

pub async fn info(app: &AppHandle, url: &str) -> Result<Info, String> {
    let mut args: Vec<String> = ["--no-playlist", "--skip-download"]
        .map(String::from)
        .to_vec();
    args.extend(youtube_args(url));
    dump_json(app, args, url).await
}

/// yt-dlp's JSON for `url` (`--dump-single-json` plus `extra` arguments).
pub async fn dump_json<T: serde::de::DeserializeOwned>(
    app: &AppHandle,
    extra: Vec<String>,
    url: &str,
) -> Result<T, String> {
    let mut args: Vec<String> = [
        "--ignore-config",
        "--encoding",
        "utf-8",
        "--no-warnings",
        "--dump-single-json",
    ]
    .map(String::from)
    .to_vec();
    args.extend(extra);
    args.push("--".into());
    args.push(url.into());
    let mut task = engine::ytdlp(app, args).await?;
    let (mut json, mut error) = (String::new(), None);
    while let Some(line) = task.next_line().await {
        match line {
            Line::Out(l) => json.push_str(&l),
            Line::Err(l) => {
                if let Some(msg) = l.strip_prefix("ERROR:") {
                    error = Some(clean_error(msg));
                }
            }
        }
    }
    if !task.success() || json.is_empty() {
        return Err(error.unwrap_or_else(|| "No se pudo leer el vídeo".into()));
    }
    serde_json::from_str(&json).map_err(|e| format!("Respuesta inesperada de yt-dlp: {e}"))
}

/// The subtitle track to transcribe, in the video's own language whenever
/// possible: hand-made subtitles first, then the automatic captions of the
/// original audio, and only then other languages. `lang` ("es") wins when the
/// video has it.
fn pick_track<'a>(info: &'a Info, lang: Option<&str>) -> Option<&'a Track> {
    let vtt = |tracks: &'a Vec<Track>| tracks.iter().find(|t| t.ext == "vtt" && !t.url.is_empty());
    // "es-orig" in the automatic captions marks the language that is spoken.
    let spoken = info
        .automatic_captions
        .keys()
        .find_map(|k| k.strip_suffix("-orig"));
    let wanted: Vec<&str> = [lang, info.language.as_deref(), spoken, Some("en")]
        .into_iter()
        .flatten()
        .filter(|l| !l.is_empty())
        .collect();
    let same = |key: &str, l: &str| key == l || key.split('-').next() == Some(l);
    let manual = |l: &str| {
        info.subtitles
            .iter()
            .filter(|(k, _)| same(k, l))
            .find_map(|(_, t)| vtt(t))
    };
    let auto = |l: &str| {
        let orig = format!("{l}-orig");
        info.automatic_captions
            .get(&orig)
            .and_then(vtt)
            .or_else(|| info.automatic_captions.get(l).and_then(vtt))
    };
    wanted
        .iter()
        .find_map(|l| manual(l).or_else(|| auto(l)))
        .or_else(|| {
            info.subtitles
                .iter()
                .filter(|(k, _)| *k != "live_chat")
                .find_map(|(_, t)| vtt(t))
        })
}

pub struct TranscriptRequest<'a> {
    pub url: &'a str,
    pub format: Format,
    pub lang: Option<&'a str>,
    pub section: Option<Section>,
    pub note: &'a str,
    pub folder: &'a Path,
}

pub async fn save_transcript(
    app: &AppHandle,
    req: TranscriptRequest<'_>,
) -> Result<PathBuf, String> {
    let info = info(app, req.url).await?;
    let track =
        pick_track(&info, req.lang).ok_or("Este vídeo no tiene subtítulos ni transcripción")?;
    let vtt = subtitles(&track.url, req.section).await?;
    let lines = transcript::clip(transcript::parse_vtt(&vtt), req.section);
    if lines.is_empty() {
        return Err("La transcripción está vacía en ese tramo".into());
    }
    let date = info
        .upload_date
        .as_deref()
        .filter(|d| d.len() == 8)
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]))
        .unwrap_or_default();
    let meta = Meta {
        title: &info.title,
        channel: info
            .channel
            .as_deref()
            .or(info.uploader.as_deref())
            .unwrap_or(""),
        date: &date,
        url: info.webpage_url.as_deref().unwrap_or(req.url),
        note: req.note,
    };
    let offset = req.section.map_or(0.0, |s| s.start);
    let text = transcript::render(&lines, req.format, &meta, offset);
    let name = format!("{}.{}", info.stem(req.section), req.format.ext());
    write_new(req.folder, &name, text.as_bytes())
}

/// Saves the best thumbnail as JPEG (or PNG/WebP when that is all there is).
pub async fn save_thumbnail(app: &AppHandle, url: &str, folder: &Path) -> Result<PathBuf, String> {
    let info = info(app, url).await?;
    let ext_of = |u: &str| {
        let path = u.split(['?', '#']).next().unwrap_or("");
        path.rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default()
    };
    // yt-dlp lists thumbnails from worst to best; the biggest ones don't
    // exist for every video, so try them in order, JPEG first.
    let mut candidates: Vec<String> = info
        .thumbnails
        .iter()
        .rev()
        .map(|t| t.url.clone())
        .collect();
    candidates.sort_by_key(|u| !matches!(ext_of(u).as_str(), "jpg" | "jpeg"));
    candidates.extend(info.thumbnail.clone());
    let mut found = None;
    let mut error = "Este vídeo no tiene miniatura".to_string();
    for url in candidates.iter().filter(|u| u.starts_with("http")) {
        match fetch(url).await {
            Ok(bytes) => {
                found = Some((url.clone(), bytes));
                break;
            }
            Err(e) => error = e,
        }
    }
    let (best, bytes) = found.ok_or(error)?;
    let ext = match ext_of(&best).as_str() {
        "png" => "png",
        "webp" => "webp",
        _ => "jpg",
    };
    let path = write_new(folder, &format!("{}.{ext}", info.stem(None)), &bytes)?;
    if ext == "webp" {
        return Ok(webp_to_jpg(app, &path).await.unwrap_or(path));
    }
    Ok(path)
}

/// Editors don't all read WebP; FFmpeg is already there to convert it.
async fn webp_to_jpg(app: &AppHandle, webp: &Path) -> Option<PathBuf> {
    let tools = crate::tools::ensure(app).await.ok()?;
    let jpg = free_name(&webp.with_extension("jpg"));
    let mut cmd = tokio::process::Command::new(tools.ffmpeg());
    cmd.args(["-y", "-loglevel", "error", "-i"])
        .arg(webp)
        .args(["-q:v", "2"])
        .arg(&jpg)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::platform::prepare(&mut cmd);
    let ok = cmd.status().await.is_ok_and(|s| s.success()) && jpg.is_file();
    if ok {
        let _ = std::fs::remove_file(webp);
        Some(jpg)
    } else {
        None
    }
}

/// One subtitle track as WebVTT text. Streaming sites (Kick) cut their
/// subtitles into HLS segments; then only the segments of `section` (or all
/// of them) are fetched and joined.
async fn subtitles(url: &str, section: Option<Section>) -> Result<String, String> {
    use futures_util::StreamExt;

    let text = String::from_utf8_lossy(&fetch(url).await?).into_owned();
    if !text.starts_with("#EXTM3U") {
        return Ok(text);
    }
    let base = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    let wanted = |start: f64, end: f64| {
        section.map_or(true, |s| end >= s.start - 1.0 && start <= s.end + 1.0)
    };
    let segments: Vec<String> = hls_segments(&text)
        .into_iter()
        .filter(|(start, end, _)| wanted(*start, *end))
        .filter_map(|(_, _, uri)| base.join(&uri).ok().map(String::from))
        .collect();
    let parts: Vec<Result<Vec<u8>, String>> = futures_util::stream::iter(segments)
        .map(|u| async move { fetch(&u).await })
        .buffered(16)
        .collect()
        .await;
    let mut vtt = String::new();
    for part in parts {
        vtt.push_str(&String::from_utf8_lossy(&part?));
        vtt.push_str("\n\n");
    }
    Ok(vtt)
}

/// (start, end, uri) of each segment in an HLS media playlist.
fn hls_segments(playlist: &str) -> Vec<(f64, f64, String)> {
    let mut out = Vec::new();
    let (mut t, mut length) = (0.0, 0.0);
    for line in playlist.lines().map(str::trim) {
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            length = rest
                .split(',')
                .next()
                .and_then(|d| d.parse().ok())
                .unwrap_or(0.0);
        } else if !line.is_empty() && !line.starts_with('#') {
            out.push((t, t + length, line.to_string()));
            t += length;
        }
    }
    out
}

/// Some CDNs (Kick's) refuse requests without a browser user agent.
pub(crate) fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0 Safari/537.36")
            .build()
            .unwrap_or_default()
    })
}

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let resp = client()
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("No se pudo descargar: {e}"))?;
    resp.bytes()
        .await
        .map(|b| b.to_vec())
        .map_err(|e| format!("No se pudo descargar: {e}"))
}

/// "name (2).ext" when "name.ext" is taken; nothing is ever overwritten.
fn free_name(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = path.extension().map(|e| e.to_string_lossy().into_owned());
    (2..)
        .map(|n| {
            let name = match &ext {
                Some(ext) => format!("{stem} ({n}).{ext}"),
                None => format!("{stem} ({n})"),
            };
            path.with_file_name(name)
        })
        .find(|p| !p.exists())
        .expect("some free name")
}

fn write_new(folder: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|e| format!("No se pudo crear la carpeta: {e}"))?;
    let path = free_name(&folder.join(name));
    std::fs::write(&path, bytes).map_err(|e| format!("No se pudo guardar el archivo: {e}"))?;
    Ok(path)
}

/// Fields of a JSON request body, for `bridge`.
pub fn str_field<'a>(body: &'a Value, key: &str) -> Option<&'a str> {
    body.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(key: &str, ext: &str) -> Vec<Track> {
        vec![Track {
            ext: ext.into(),
            url: key.into(),
        }]
    }

    #[test]
    fn hls_segments_have_times() {
        let playlist = "#EXTM3U
#EXT-X-TARGETDURATION:13
#EXTINF:11.5,
0.vtt
#EXT-X-PROGRAM-DATE-TIME:x
#EXTINF:10,
1.vtt
#EXT-X-ENDLIST
";
        assert_eq!(
            hls_segments(playlist),
            [
                (0.0, 11.5, "0.vtt".to_string()),
                (11.5, 21.5, "1.vtt".to_string())
            ]
        );
    }

    #[test]
    fn the_original_language_wins() {
        let mut info = Info {
            language: Some("es".into()),
            ..Info::default()
        };
        for key in ["en", "es-orig", "es"] {
            info.automatic_captions
                .insert(key.into(), track(key, "vtt"));
        }
        let picked = |info: &Info, lang| pick_track(info, lang).map(|t| t.url.clone());
        assert_eq!(picked(&info, None).as_deref(), Some("es-orig"));
        assert_eq!(picked(&info, Some("en")).as_deref(), Some("en"));
        // A translation into another language loses against the spoken one.
        info.subtitles.insert("de".into(), track("de", "vtt"));
        assert_eq!(picked(&info, None).as_deref(), Some("es-orig"));
        info.subtitles
            .insert("es-419".into(), track("manual-es", "vtt"));
        assert_eq!(picked(&info, None).as_deref(), Some("manual-es"));
        info.language = None;
        info.automatic_captions.clear();
        assert_eq!(picked(&info, None).as_deref(), Some("de"));
        info.subtitles.clear();
        info.automatic_captions.clear();
        info.automatic_captions
            .insert("de".into(), track("de", "json3"));
        assert!(pick_track(&info, None).is_none());
    }
}

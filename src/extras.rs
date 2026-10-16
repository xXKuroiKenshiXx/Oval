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
    let mut args: Vec<String> = [
        "--ignore-config",
        "--encoding",
        "utf-8",
        "--no-warnings",
        "--dump-single-json",
        "--no-playlist",
        "--skip-download",
    ]
    .map(String::from)
    .to_vec();
    args.extend(youtube_args(url));
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
    let vtt = fetch(&track.url).await?;
    let lines = transcript::clip(
        transcript::parse_vtt(&String::from_utf8_lossy(&vtt)),
        req.section,
    );
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
    for url in candidates.iter().filter(|u| u.starts_with("http")) {
        if let Ok(bytes) = fetch(url).await {
            found = Some((url.clone(), bytes));
            break;
        }
    }
    let (best, bytes) = found.ok_or("Este vídeo no tiene miniatura")?;
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

async fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let resp = reqwest::get(url)
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

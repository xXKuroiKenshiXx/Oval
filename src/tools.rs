//! Locates (and on first run fetches) the download engine: yt-dlp and FFmpeg.
//!
//! Everything lives in the user's local data dir (`%LOCALAPPDATA%\Oval\bin`,
//! `~/.local/share/Oval/bin`). yt-dlp is the "onedir" build because it starts
//! several times faster than the single-file one, which matters when many
//! downloads are queued. A system FFmpeg is used when there is one.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use futures_util::StreamExt;
use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::Mutex;

const YTDLP_API: &str = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const UPDATE_EVERY: Duration = Duration::from_secs(60 * 60 * 24);

#[cfg(windows)]
mod target {
    pub const YTDLP_ARCHIVE: &str =
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_win.zip";
    pub const YTDLP_NAMES: &[&str] = &["yt-dlp.exe"];
    pub const FFMPEG_ARCHIVE: &str =
        "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl-shared.zip";
    pub const FFMPEG: &str = "ffmpeg.exe";
}

#[cfg(not(windows))]
mod target {
    pub const YTDLP_ARCHIVE: &str =
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux.zip";
    pub const YTDLP_NAMES: &[&str] = &["yt-dlp_linux", "yt-dlp"];
    pub const FFMPEG_ARCHIVE: &str =
        "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-linux64-gpl.tar.xz";
    pub const FFMPEG: &str = "ffmpeg";
}

pub use target::FFMPEG;

#[derive(Clone, Debug)]
pub struct Tools {
    pub ytdlp: PathBuf,
    /// `None` when ffmpeg is already reachable through PATH.
    pub ffmpeg_dir: Option<PathBuf>,
}

impl Tools {
    pub fn ffmpeg(&self) -> PathBuf {
        match &self.ffmpeg_dir {
            Some(dir) => dir.join(FFMPEG),
            None => PathBuf::from(FFMPEG),
        }
    }
}

#[derive(Clone, Serialize)]
pub struct EngineStatus {
    pub ready: bool,
    pub message: String,
    pub progress: Option<f64>,
}

static TOOLS: Mutex<Option<Tools>> = Mutex::const_new(None);
static LAST: std::sync::Mutex<Option<EngineStatus>> = std::sync::Mutex::new(None);

/// The latest engine status, for a UI that subscribed after it was emitted.
pub fn status() -> EngineStatus {
    LAST.lock().unwrap().clone().unwrap_or(EngineStatus {
        ready: false,
        message: "Preparando motor".into(),
        progress: None,
    })
}

pub fn bin_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("Oval")
        .join("bin")
}

fn emit(app: &AppHandle, ready: bool, message: &str, progress: Option<f64>) {
    let status = EngineStatus {
        ready,
        message: message.into(),
        progress,
    };
    *LAST.lock().unwrap() = Some(status.clone());
    let _ = app.emit("engine", status);
}

fn find_ytdlp(home: &Path) -> Option<PathBuf> {
    target::YTDLP_NAMES.iter().find_map(|n| find_exe(home, n))
}

/// Returns the tools, downloading whatever is missing. Concurrent callers wait
/// on the same lock, so the engine is only fetched once.
pub async fn ensure(app: &AppHandle) -> Result<Tools, String> {
    let mut guard = TOOLS.lock().await;
    if let Some(t) = guard.as_ref() {
        return Ok(t.clone());
    }
    let dir = bin_dir();
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;

    let ytdlp_home = dir.join("yt-dlp");
    let mut ytdlp = find_ytdlp(&ytdlp_home);
    if ytdlp.is_none() {
        emit(app, false, "Preparando motor", Some(0.0));
        match install(
            app,
            target::YTDLP_ARCHIVE,
            &ytdlp_home,
            |_| true,
            "Preparando motor",
        )
        .await
        {
            Ok(()) => {
                ytdlp = find_ytdlp(&ytdlp_home);
                if let Ok(tag) = latest_tag().await {
                    let _ = std::fs::write(dir.join("yt-dlp.version"), tag);
                }
            }
            Err(e) => {
                // Offline on first run: fall back to a system copy if there is one.
                ytdlp = which(if cfg!(windows) {
                    "yt-dlp.exe"
                } else {
                    "yt-dlp"
                });
                if ytdlp.is_none() {
                    emit(app, false, "Sin conexión para preparar el motor", None);
                    return Err(e);
                }
            }
        }
    }
    let ytdlp = ytdlp.ok_or("yt-dlp no encontrado")?;

    let ffmpeg_home = dir.join("ffmpeg");
    let ffmpeg_dir = if let Some(p) = find_exe(&ffmpeg_home, FFMPEG) {
        p.parent().map(Path::to_path_buf)
    } else if which(FFMPEG).is_some() {
        None
    } else {
        emit(app, false, "Preparando FFmpeg", Some(0.0));
        let keep = |name: &str| {
            let lower = name.to_ascii_lowercase();
            let file = lower.rsplit('/').next().unwrap_or("");
            lower.contains("/bin/")
                && (file.ends_with(".exe")
                    || file.ends_with(".dll")
                    || file == "ffmpeg"
                    || file == "ffprobe")
        };
        install(
            app,
            target::FFMPEG_ARCHIVE,
            &ffmpeg_home,
            keep,
            "Preparando FFmpeg",
        )
        .await?;
        find_exe(&ffmpeg_home, FFMPEG).and_then(|p| p.parent().map(Path::to_path_buf))
    };

    let tools = Tools { ytdlp, ffmpeg_dir };
    *guard = Some(tools.clone());
    emit(app, true, "Listo", None);
    Ok(tools)
}

/// Refreshes the managed yt-dlp once a day. Sites change constantly and an old
/// extractor is the most common reason a download fails.
pub async fn update_in_background(app: AppHandle) {
    let dir = bin_dir();
    let stamp = dir.join("yt-dlp.version");
    let fresh = std::fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < UPDATE_EVERY);
    let home = dir.join("yt-dlp");
    if fresh || find_ytdlp(&home).is_none() {
        return;
    }
    let Ok(tag) = latest_tag().await else { return };
    let current = std::fs::read_to_string(&stamp).unwrap_or_default();
    if current.trim() == tag {
        let _ = std::fs::write(&stamp, &tag); // touch
        return;
    }
    // Hold the lock so no download starts against a half-replaced engine.
    let mut guard = TOOLS.lock().await;
    let staging = dir.join("yt-dlp.new");
    let _ = std::fs::remove_dir_all(&staging);
    if install(
        &app,
        target::YTDLP_ARCHIVE,
        &staging,
        |_| true,
        "Actualizando motor",
    )
    .await
    .is_ok()
    {
        let old = dir.join("yt-dlp.old");
        let _ = std::fs::remove_dir_all(&old);
        if std::fs::rename(&home, &old).is_ok() && std::fs::rename(&staging, &home).is_ok() {
            let _ = std::fs::remove_dir_all(&old);
            let _ = std::fs::write(&stamp, &tag);
            if let (Some(t), Some(p)) = (guard.as_mut(), find_ytdlp(&home)) {
                t.ytdlp = p;
            }
        } else if !home.exists() {
            let _ = std::fs::rename(&old, &home);
        }
    }
    let _ = std::fs::remove_dir_all(&staging);
    emit(&app, guard.is_some(), "Listo", None);
}

async fn latest_tag() -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Release {
        tag_name: String,
    }
    let r: Release = client()
        .get(YTDLP_API)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(r.tag_name)
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Oval/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

/// Downloads an archive into memory with progress events and extracts the
/// entries accepted by `keep` into `dest`, flattening FFmpeg's `bin/` folder.
async fn install(
    app: &AppHandle,
    url: &str,
    dest: &Path,
    keep: impl Fn(&str) -> bool + Send + 'static,
    label: &str,
) -> Result<(), String> {
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let total = resp.content_length().unwrap_or(0) as f64;
    let mut bytes = Vec::with_capacity(total as usize);
    let mut stream = resp.bytes_stream();
    let mut last = 0.0;
    while let Some(chunk) = stream.next().await {
        bytes.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        if total > 0.0 {
            let p = bytes.len() as f64 / total;
            if p - last > 0.01 {
                last = p;
                emit(app, false, label, Some(p));
            }
        }
    }

    let dest = dest.to_path_buf();
    let tar_xz = url.ends_with(".tar.xz");
    tokio::task::spawn_blocking(move || -> Result<(), String> {
        std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
        if tar_xz {
            extract_tar_xz(&bytes, &dest, &keep)
        } else {
            extract_zip(bytes, &dest, &keep)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Where an archive entry lands: FFmpeg's `.../bin/x` becomes `x`.
fn entry_path(dest: &Path, name: &str) -> Option<PathBuf> {
    let rel = match name.find("/bin/") {
        Some(at) => PathBuf::from(&name[at + 5..]),
        None => PathBuf::from(name),
    };
    let safe = rel
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    (safe && !rel.as_os_str().is_empty()).then(|| dest.join(rel))
}

fn write_entry(
    out: &Path,
    reader: &mut impl std::io::Read,
    mode: Option<u32>,
) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut w = std::fs::File::create(out).map_err(|e| e.to_string())?;
    std::io::copy(reader, &mut w).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    if let Some(mode) = mode {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(out, std::fs::Permissions::from_mode(mode | 0o600));
    }
    #[cfg(not(unix))]
    let _ = mode;
    Ok(())
}

fn extract_zip(bytes: Vec<u8>, dest: &Path, keep: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().replace('\\', "/");
        if file.is_dir() || !keep(&name) {
            continue;
        }
        let Some(out) = entry_path(dest, &name) else {
            continue;
        };
        let mode = file.unix_mode();
        write_entry(&out, &mut file, mode)?;
    }
    Ok(())
}

#[cfg(not(windows))]
fn extract_tar_xz(bytes: &[u8], dest: &Path, keep: &dyn Fn(&str) -> bool) -> Result<(), String> {
    let mut tar_bytes = Vec::new();
    lzma_rs::xz_decompress(&mut std::io::Cursor::new(bytes), &mut tar_bytes)
        .map_err(|e| e.to_string())?;
    let mut archive = tar::Archive::new(std::io::Cursor::new(tar_bytes));
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let name = entry
            .path()
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        if !keep(&name) {
            continue;
        }
        let Some(out) = entry_path(dest, &name) else {
            continue;
        };
        let mode = entry.header().mode().ok();
        write_entry(&out, &mut entry, mode)?;
    }
    Ok(())
}

#[cfg(windows)]
fn extract_tar_xz(_: &[u8], _: &Path, _: &dyn Fn(&str) -> bool) -> Result<(), String> {
    Err("tar.xz is not used on Windows".into())
}

/// Finds `exe` directly in `dir` or one level below it.
fn find_exe(dir: &Path, exe: &str) -> Option<PathBuf> {
    let direct = dir.join(exe);
    if direct.is_file() {
        return Some(direct);
    }
    std::fs::read_dir(dir).ok()?.flatten().find_map(|e| {
        let p = e.path().join(exe);
        p.is_file().then_some(p)
    })
}

fn which(exe: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(exe))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::entry_path;
    use std::path::Path;

    #[test]
    fn archive_paths_are_flattened_and_safe() {
        let d = Path::new("dest");
        assert_eq!(
            entry_path(d, "ffmpeg-x/bin/ffmpeg.exe"),
            Some(d.join("ffmpeg.exe"))
        );
        assert_eq!(
            entry_path(d, "_internal/a.dll"),
            Some(d.join("_internal/a.dll"))
        );
        assert_eq!(entry_path(d, "../evil"), None);
    }
}

//! The download queue: every job is one yt-dlp process. A tiny scheduler keeps
//! at most `limit` of them running; pause stops the process and resume starts
//! it again, letting yt-dlp continue from its `.part` files.
//!
//! Live recordings are jobs too, but they skip the limit (a stream can run for
//! hours) and "stop" keeps what was recorded instead of throwing it away.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::{platform, settings, tools};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Video,
    Audio,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Queued,
    Starting,
    Downloading,
    Processing,
    Paused,
    Done,
    Error,
}

impl Status {
    fn is_active(self) -> bool {
        matches!(
            self,
            Status::Starting | Status::Downloading | Status::Processing
        )
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: u64,
    pub url: String,
    pub mode: Mode,
    /// Max video height ("1080") or audio bitrate in kbps ("192"); `None` = best.
    pub quality: Option<String>,
    pub title: String,
    pub thumbnail: Option<String>,
    pub status: Status,
    /// 0..1, monotonic across the video and audio streams.
    pub progress: f64,
    pub downloaded: f64,
    pub total: f64,
    pub speed: f64,
    pub eta: Option<f64>,
    pub error: Option<String>,
    pub path: Option<String>,
    /// A live stream being recorded in real time.
    pub live: bool,
    /// Seconds recorded so far (live only).
    pub elapsed: f64,
    #[serde(skip)]
    dir: PathBuf,
    /// Set by "stop" on a live recording: finish and keep the file.
    #[serde(skip)]
    stopping: bool,
    /// File yt-dlp is writing to (live only, known once recording starts).
    #[serde(skip)]
    raw: Option<PathBuf>,
    #[serde(skip)]
    stem: Option<String>,
    #[serde(skip)]
    pid: Option<u32>,
    /// Streams in the selected format ("137+140" = 2) and how many finished.
    #[serde(skip)]
    parts: u32,
    #[serde(skip)]
    parts_done: u32,
    /// Bumped on every (re)start so a stale process can't update the job.
    #[serde(skip)]
    run: u64,
}

#[derive(Deserialize)]
pub struct NewJob {
    pub url: String,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
}

#[derive(Default)]
pub struct Queue {
    jobs: Mutex<Vec<Job>>,
    next_id: Mutex<u64>,
}

pub fn download_root(app: &AppHandle) -> PathBuf {
    if let Some(dir) = settings::get(app).folder {
        return dir;
    }
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("Oval")
}

fn queue(app: &AppHandle) -> tauri::State<'_, Queue> {
    app.state::<Queue>()
}

fn emit_job(app: &AppHandle, job: &Job) {
    let _ = app.emit("job", job);
}

impl Queue {
    pub fn list(&self) -> Vec<Job> {
        self.jobs.lock().unwrap().clone()
    }

    pub fn add(
        &self,
        app: &AppHandle,
        items: Vec<NewJob>,
        mode: Mode,
        quality: Option<&str>,
        folder: Option<&str>,
        live: bool,
    ) -> Vec<Job> {
        let quality = quality.filter(|q| !q.is_empty() && q.bytes().all(|b| b.is_ascii_digit()));
        let mut dir = download_root(app);
        if let Some(f) = folder.map(sanitize).filter(|f| !f.is_empty()) {
            dir.push(f);
        }
        let mut added = Vec::new();
        {
            let mut jobs = self.jobs.lock().unwrap();
            let mut next = self.next_id.lock().unwrap();
            for item in items {
                *next += 1;
                let url = item.url.trim().to_string();
                let job = Job {
                    id: *next,
                    title: item
                        .title
                        .filter(|t| !t.is_empty())
                        .unwrap_or_else(|| short_url(&url)),
                    url,
                    mode,
                    quality: quality.map(str::to_string),
                    thumbnail: item.thumbnail,
                    status: Status::Queued,
                    progress: 0.0,
                    downloaded: 0.0,
                    total: 0.0,
                    speed: 0.0,
                    eta: None,
                    error: None,
                    path: None,
                    live,
                    elapsed: 0.0,
                    dir: dir.clone(),
                    stopping: false,
                    raw: None,
                    stem: None,
                    pid: None,
                    parts: 1,
                    parts_done: 0,
                    run: 0,
                };
                emit_job(app, &job);
                added.push(job.clone());
                jobs.push(job);
            }
        }
        schedule(app);
        added
    }

    pub fn pause(&self, app: &AppHandle, id: u64) {
        let pid = self.update(app, id, |j| {
            if j.status.is_active() || j.status == Status::Queued {
                j.status = Status::Paused;
                j.speed = 0.0;
                j.eta = None;
                j.run += 1;
                j.pid.take()
            } else {
                None
            }
        });
        if let Some(pid) = pid.flatten() {
            platform::kill_tree(pid);
        }
        schedule(app);
    }

    /// Ends a live recording and keeps everything captured so far.
    pub fn stop_live(&self, app: &AppHandle, id: u64) {
        let pid = self.update(app, id, |j| {
            if j.live && j.status.is_active() {
                j.stopping = true;
                j.status = Status::Processing;
                j.speed = 0.0;
                j.pid
            } else {
                None
            }
        });
        if let Some(pid) = pid.flatten() {
            platform::kill_tree(pid);
        }
    }

    pub fn resume(&self, app: &AppHandle, id: u64) {
        self.update(app, id, |j| {
            if matches!(j.status, Status::Paused | Status::Error) {
                j.status = Status::Queued;
                j.error = None;
                j.stopping = false;
            }
        });
        schedule(app);
    }

    /// Stops the job, deletes its partial files and drops it from the list.
    pub fn cancel(&self, app: &AppHandle, id: u64) {
        let removed = {
            let mut jobs = self.jobs.lock().unwrap();
            jobs.iter().position(|j| j.id == id).map(|i| jobs.remove(i))
        };
        if let Some(job) = removed {
            let _ = app.emit("job-removed", id);
            let finished = job.status == Status::Done;
            tauri::async_runtime::spawn(async move {
                if let Some(pid) = job.pid {
                    platform::kill_tree(pid);
                    // Give Windows a moment to release the file handles.
                    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                }
                if !finished {
                    remove_partials(&job.dir, job.stem.as_deref());
                    if let Some(raw) = &job.raw {
                        let _ = std::fs::remove_file(raw);
                    }
                }
            });
        }
        schedule(app);
    }

    pub fn clear_finished(&self, app: &AppHandle) {
        let mut jobs = self.jobs.lock().unwrap();
        jobs.retain(|j| {
            let keep = j.status != Status::Done;
            if !keep {
                let _ = app.emit("job-removed", j.id);
            }
            keep
        });
    }

    pub fn path_of(&self, id: u64) -> Option<PathBuf> {
        let jobs = self.jobs.lock().unwrap();
        let job = jobs.iter().find(|j| j.id == id)?;
        Some(
            job.path
                .as_ref()
                .map(PathBuf::from)
                .unwrap_or_else(|| job.dir.clone()),
        )
    }

    /// Applies `f` to a job and broadcasts the new state.
    fn update<R>(&self, app: &AppHandle, id: u64, f: impl FnOnce(&mut Job) -> R) -> Option<R> {
        let mut jobs = self.jobs.lock().unwrap();
        let job = jobs.iter_mut().find(|j| j.id == id)?;
        let r = f(job);
        emit_job(app, job);
        Some(r)
    }

    /// Like `update`, but ignored if the job was restarted or paused since `run`.
    fn update_run(&self, app: &AppHandle, id: u64, run: u64, f: impl FnOnce(&mut Job)) {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.iter_mut().find(|j| j.id == id && j.run == run) {
            f(job);
            emit_job(app, job);
        }
    }
}

/// Starts queued jobs until the concurrency limit is reached. Live recordings
/// start right away and don't count towards the limit.
pub fn schedule(app: &AppHandle) {
    let limit = settings::get(app).concurrency.clamp(1, 8) as usize;
    let to_start: Vec<(u64, u64)> = {
        let q = queue(app);
        let mut jobs = q.jobs.lock().unwrap();
        let mut running = jobs
            .iter()
            .filter(|j| !j.live && j.status.is_active())
            .count();
        let mut out = Vec::new();
        for job in jobs.iter_mut() {
            if job.status != Status::Queued || (!job.live && running >= limit) {
                continue;
            }
            job.status = Status::Starting;
            job.run += 1;
            job.speed = 0.0;
            job.eta = None;
            job.parts_done = 0;
            job.elapsed = 0.0;
            job.raw = None;
            if !job.live {
                running += 1;
            }
            emit_job(app, job);
            out.push((job.id, job.run));
        }
        out
    };
    for (id, run) in to_start {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = run_job(&app, id, run).await {
                queue(&app).update_run(&app, id, run, |j| {
                    j.status = Status::Error;
                    j.error = Some(e);
                    j.pid = None;
                    j.speed = 0.0;
                });
            }
            schedule(&app);
        });
    }
}

async fn run_job(app: &AppHandle, id: u64, run: u64) -> Result<(), String> {
    let tools = tools::ensure(app).await?;
    let (url, mode, quality, dir, live) = {
        let q = queue(app);
        let jobs = q.jobs.lock().unwrap();
        match jobs.iter().find(|j| j.id == id && j.run == run) {
            Some(j) => (
                j.url.clone(),
                j.mode,
                j.quality.clone(),
                j.dir.clone(),
                j.live,
            ),
            None => return Ok(()), // paused or canceled while the engine loaded
        }
    };
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;

    let mut cmd = Command::new(&tools.ytdlp);
    cmd.args([
        "--ignore-config",
        "--no-warnings",
        "--newline",
        "--progress",
        "--progress-delta",
        "0.25",
        "--progress-template",
        "download:OVALP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s",
        "--no-simulate",
        "--print",
        "before_dl:OVALM|%(format_id)s|%(thumbnail)s|%(filename)s|%(title)s",
        "--print",
        "after_move:OVALF|%(filepath)s",
        "--no-playlist",
        "--no-mtime",
        "--concurrent-fragments",
        "8",
    ]);
    cmd.arg("-P").arg(&dir);
    if let Some(ff) = &tools.ffmpeg_dir {
        cmd.arg("--ffmpeg-location").arg(ff);
    }
    match mode {
        _ if live => {
            // Recorded as MPEG-TS straight into the final name: a TS file stays
            // playable when the process is cut, and "stop" remuxes it to MP4.
            let sort = match &quality {
                Some(h) => format!("res:{h},vcodec:h264,acodec:aac"),
                None => "res,vcodec:h264,acodec:aac".into(),
            };
            cmd.args([
                "-o",
                "%(uploader,channel,id).80B %(epoch>%Y-%m-%d %H-%M-%S)s.%(ext)s",
                "-f",
                "b/bv*+ba",
                "-S",
                &sort,
                "--hls-use-mpegts",
                "--no-part",
            ]);
        }
        Mode::Video => {
            cmd.args(["-o", "%(title).150B [%(id)s].%(ext)s"]);
            // "res:1080" = the best stream no taller than 1080p. H.264/AAC wins ties so
            // files open everywhere, editors included; above 1080p YouTube has no H.264.
            let sort = match &quality {
                Some(h) => format!("res:{h},vcodec:h264,acodec:aac"),
                None => "res,vcodec:h264,acodec:aac".into(),
            };
            cmd.args([
                "-f",
                "bv*+ba/b",
                "-S",
                &sort,
                "--merge-output-format",
                "mp4",
            ]);
        }
        Mode::Audio => {
            cmd.args(["-o", "%(title).150B [%(id)s].%(ext)s"]);
            let bitrate = quality
                .as_ref()
                .map_or_else(|| "0".into(), |k| format!("{k}K"));
            cmd.args([
                "-f",
                "ba/b",
                "-x",
                "--audio-format",
                "mp3",
                "--audio-quality",
                &bitrate,
            ]);
        }
    }
    cmd.arg("--").arg(&url);
    cmd.env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    platform::prepare(&mut cmd);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("No se pudo iniciar yt-dlp: {e}"))?;
    platform::adopt_child(&child);
    let pid = child.id();
    let q = queue(app);
    let mut still_ours = false;
    q.update_run(app, id, run, |j| {
        j.pid = pid;
        still_ours = true;
    });
    if !still_ours {
        // Paused between scheduling and spawning.
        let _ = child.start_kill();
        return Ok(());
    }

    let stderr = child.stderr.take().expect("piped stderr");
    let err_task = tokio::spawn(async move {
        let mut last = None;
        let mut lines = BufReader::new(stderr).split(b'\n');
        while let Ok(Some(raw)) = lines.next_segment().await {
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if let Some(msg) = line.strip_prefix("ERROR:") {
                last = Some(clean_error(msg));
            }
        }
        last
    });

    let finished = Arc::new(AtomicBool::new(false));
    if live {
        tokio::spawn(watch_recording(app.clone(), id, run, finished.clone()));
    }

    let stdout = child.stdout.take().expect("piped stdout");
    let mut lines = BufReader::new(stdout).split(b'\n');
    while let Ok(Some(raw)) = lines.next_segment().await {
        let line = String::from_utf8_lossy(&raw);
        handle_line(app, id, run, live, line.trim());
    }

    let status = child.wait().await.map_err(|e| e.to_string())?;
    finished.store(true, Ordering::Relaxed);
    let error = err_task.await.ok().flatten();

    if live {
        return finish_recording(app, &tools, id, run, status.success(), error).await;
    }
    q.update_run(app, id, run, |j| {
        j.pid = None;
        j.speed = 0.0;
        j.eta = None;
        if status.success() {
            j.status = Status::Done;
            j.progress = 1.0;
            if j.total > 0.0 {
                j.downloaded = j.total;
            }
        } else {
            j.status = Status::Error;
            j.error = Some(error.unwrap_or_else(|| "La descarga falló".into()));
        }
    });
    Ok(())
}

/// Polls the size of a live recording once a second: yt-dlp hands live
/// streams to FFmpeg, which reports no usable progress of its own.
async fn watch_recording(app: AppHandle, id: u64, run: u64, finished: Arc<AtomicBool>) {
    let started = Instant::now();
    let mut last = (started, 0u64);
    while !finished.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let raw = {
            let q = queue(&app);
            let jobs = q.jobs.lock().unwrap();
            match jobs.iter().find(|j| j.id == id && j.run == run) {
                Some(j) if !j.stopping => j.raw.clone(),
                _ => return,
            }
        };
        let size = raw
            .and_then(|p| std::fs::metadata(p).ok())
            .map_or(0, |m| m.len());
        let now = Instant::now();
        let dt = now.duration_since(last.0).as_secs_f64();
        let speed = if dt > 0.0 {
            size.saturating_sub(last.1) as f64 / dt
        } else {
            0.0
        };
        last = (now, size);
        queue(&app).update_run(&app, id, run, |j| {
            if size > 0 {
                j.status = Status::Downloading;
            }
            j.downloaded = size as f64;
            j.speed = speed;
            j.elapsed = now.duration_since(started).as_secs_f64();
        });
    }
}

/// Wraps up a live recording that stopped for any reason: whatever made it to
/// disk is kept, so a dropped connection doesn't lose the stream.
async fn finish_recording(
    app: &AppHandle,
    tools: &tools::Tools,
    id: u64,
    run: u64,
    success: bool,
    error: Option<String>,
) -> Result<(), String> {
    let (raw, stopping) = {
        let q = queue(app);
        let jobs = q.jobs.lock().unwrap();
        match jobs.iter().find(|j| j.id == id && j.run == run) {
            Some(j) => (j.raw.clone(), j.stopping),
            None => return Ok(()), // canceled
        }
    };
    let Some(raw) = raw.filter(|p| std::fs::metadata(p).is_ok_and(|m| m.len() > 0)) else {
        return Err(error.unwrap_or_else(|| "No se pudo grabar el directo".into()));
    };
    let q = queue(app);
    q.update_run(app, id, run, |j| {
        j.status = Status::Processing;
        j.pid = None;
        j.speed = 0.0;
    });
    // A stream that ended on its own was already fixed up by yt-dlp.
    let path = if success && !stopping {
        raw
    } else {
        tokio::time::sleep(Duration::from_millis(500)).await; // let Windows release the file
        remux_to_mp4(tools, &raw).await
    };
    let size = std::fs::metadata(&path).map_or(0.0, |m| m.len() as f64);
    q.update_run(app, id, run, |j| {
        j.status = Status::Done;
        j.progress = 1.0;
        j.downloaded = size;
        j.total = size;
        j.path = Some(path.display().to_string());
    });
    Ok(())
}

/// Turns the MPEG-TS recording into a regular MP4 without re-encoding. If
/// FFmpeg fails the `.ts` file is kept, which every player opens anyway.
async fn remux_to_mp4(tools: &tools::Tools, raw: &Path) -> PathBuf {
    let ts = raw.with_extension("ts");
    if std::fs::rename(raw, &ts).is_err() {
        return raw.to_path_buf();
    }
    let mp4 = raw.with_extension("mp4");
    let mut cmd = Command::new(tools.ffmpeg());
    cmd.args(["-y", "-loglevel", "error", "-i"])
        .arg(&ts)
        .args(["-map", "0", "-c", "copy", "-movflags", "+faststart"])
        .arg(&mp4)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    platform::prepare(&mut cmd);
    match cmd.status().await {
        Ok(s) if s.success() => {
            let _ = std::fs::remove_file(&ts);
            mp4
        }
        _ => {
            let _ = std::fs::remove_file(&mp4);
            ts
        }
    }
}

fn handle_line(app: &AppHandle, id: u64, run: u64, live: bool, line: &str) {
    let q = queue(app);
    if let Some(rest) = line.strip_prefix("OVALP|") {
        if live {
            return; // see watch_recording
        }
        let f: Vec<&str> = rest.split('|').collect();
        if f.len() < 6 {
            return;
        }
        let num = |s: &str| s.parse::<f64>().ok().filter(|v| v.is_finite());
        let (state, done) = (f[0], num(f[1]).unwrap_or(0.0));
        let total = num(f[2]).or_else(|| num(f[3])).unwrap_or(0.0);
        q.update_run(app, id, run, |j| {
            let parts = j.parts.max(1) as f64;
            // The first stream (video) dominates the size, so it gets most of the bar.
            let (base, span) = if parts > 1.0 {
                if j.parts_done == 0 {
                    (0.0, 0.9)
                } else {
                    (0.9, 0.1 / (parts - 1.0))
                }
            } else {
                (0.0, 1.0)
            };
            let base = if j.parts_done > 1 {
                base + span * (j.parts_done - 1) as f64
            } else {
                base
            };
            let local = if total > 0.0 {
                (done / total).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if state == "finished" {
                j.parts_done += 1;
                j.progress = j.progress.max(base + span);
                j.status = Status::Processing;
                j.speed = 0.0;
                j.eta = None;
            } else {
                j.status = Status::Downloading;
                j.progress = j.progress.max(base + span * local);
                if j.parts_done == 0 {
                    j.downloaded = done;
                    j.total = total;
                }
                j.speed = num(f[4]).unwrap_or(0.0);
                j.eta = num(f[5]);
            }
        });
    } else if let Some(rest) = line.strip_prefix("OVALM|") {
        let mut f = rest.splitn(4, '|');
        let (format, thumb, file, title) = (
            f.next().unwrap_or(""),
            f.next().unwrap_or("NA"),
            f.next().unwrap_or(""),
            f.next().unwrap_or(""),
        );
        q.update_run(app, id, run, |j| {
            j.parts = format.matches('+').count() as u32 + 1;
            if !title.is_empty() && title != "NA" {
                j.title = title.to_string();
            }
            if thumb.starts_with("http") {
                j.thumbnail.get_or_insert_with(|| thumb.to_string());
            }
            j.stem = Path::new(file)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned());
            if j.live && !file.is_empty() {
                j.raw = Some(PathBuf::from(file));
            }
            if j.status == Status::Starting {
                j.status = Status::Downloading;
            }
        });
    } else if let Some(path) = line.strip_prefix("OVALF|") {
        q.update_run(app, id, run, |j| j.path = Some(path.to_string()));
    }
}

fn remove_partials(dir: &Path, stem: Option<&str>) {
    let Some(stem) = stem.filter(|s| !s.is_empty()) else {
        return;
    };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_string_lossy().starts_with(stem) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
}

pub(crate) fn clean_error(msg: &str) -> String {
    let msg = msg.trim();
    // "[youtube] abc123: Video unavailable" -> "Video unavailable"
    let msg = match msg.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((_, rest)) => rest.split_once(": ").map(|(_, m)| m).unwrap_or(rest),
        None => msg,
    };
    let lower = msg.to_ascii_lowercase();
    if lower.contains("offline")
        || lower.contains("not currently live")
        || lower.contains("is not live")
    {
        "El canal no está en directo ahora".into()
    } else if lower.contains("unsupported url") {
        "Este enlace no es compatible".into()
    } else if lower.contains("private") {
        "El vídeo es privado".into()
    } else if lower.contains("sign in") || lower.contains("login") || lower.contains("cookies") {
        "El sitio pide iniciar sesión".into()
    } else if lower.contains("unavailable") {
        "El vídeo no está disponible".into()
    } else {
        msg.chars().take(140).collect()
    }
}

fn short_url(url: &str) -> String {
    let s = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    s.chars().take(60).collect()
}

pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .chars()
        .take(80)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_are_humanized() {
        assert_eq!(
            clean_error("[youtube] abc: Video unavailable"),
            "El vídeo no está disponible"
        );
        assert_eq!(
            clean_error(" Unsupported URL: https://x"),
            "Este enlace no es compatible"
        );
        assert_eq!(
            clean_error("[twitch:stream] abc: The channel is not currently live"),
            "El canal no está en directo ahora"
        );
    }

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(sanitize("A/B: C?"), "A B  C");
        assert_eq!(sanitize("name."), "name");
    }
}

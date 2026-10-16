//! The download queue: every job is one yt-dlp process. A tiny scheduler keeps
//! at most `limit` of them running; pause stops the process and resume starts
//! it again, letting yt-dlp continue from its `.part` files.
//!
//! Live recordings are jobs too, but they skip the limit (a stream can run for
//! hours) and "stop" keeps what was recorded instead of throwing it away.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::engine::{self, Line};
use crate::settings;

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
    /// Only this part of the video (a clip sent from the browser extension).
    pub section: Option<Section>,
    /// Cut the clip on the exact frame (re-encodes around the cuts).
    pub precise: bool,
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
    pid: Option<engine::Handle>,
    /// Streams in the selected format ("137+140" = 2) and how many finished.
    #[serde(skip)]
    parts: u32,
    #[serde(skip)]
    parts_done: u32,
    /// Bumped on every (re)start so a stale process can't update the job.
    #[serde(skip)]
    run: u64,
    /// Already retried once with YouTube's fallback clients.
    #[serde(skip)]
    retried: bool,
}

#[derive(Default, Deserialize)]
pub struct NewJob {
    pub url: String,
    pub title: Option<String>,
    pub thumbnail: Option<String>,
    #[serde(default)]
    pub section: Option<Section>,
    #[serde(default)]
    pub precise: bool,
    /// Absolute folder chosen outside the app (browser extension).
    #[serde(skip)]
    pub dir: Option<PathBuf>,
}

/// Start and end of a clip, in seconds.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Section {
    pub start: f64,
    pub end: f64,
}

impl Section {
    pub fn is_valid(&self) -> bool {
        self.start.is_finite() && self.end.is_finite() && self.start >= 0.0 && self.end > self.start
    }

    /// "15m04s-15m22s": readable in a file name and valid on every system.
    pub fn label(&self) -> String {
        fn t(sec: f64) -> String {
            let ms = (sec * 1000.0).round() as u64;
            let (h, m, s, frac) = (
                ms / 3_600_000,
                ms / 60_000 % 60,
                ms / 1000 % 60,
                ms % 1000 / 100,
            );
            let tail = if frac > 0 {
                format!("{s:02}.{frac}s")
            } else {
                format!("{s:02}s")
            };
            if h > 0 {
                format!("{h}h{m:02}m{tail}")
            } else {
                format!("{m}m{tail}")
            }
        }
        format!("{}-{}", t(self.start), t(self.end))
    }
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
    #[cfg(target_os = "android")]
    return PathBuf::from("/storage/emulated/0/Download/Oval");
    // The app's Documents folder: "On My iPhone > Oval" in the Files app.
    #[cfg(target_os = "ios")]
    return app
        .path()
        .document_dir()
        .unwrap_or_else(|_| std::env::temp_dir());
    #[cfg(not(mobile))]
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

    /// Whether anything is downloading or waiting to.
    #[cfg(windows)]
    pub fn busy(&self) -> bool {
        self.jobs
            .lock()
            .unwrap()
            .iter()
            .any(|j| j.status.is_active() || j.status == Status::Queued)
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
                let url = normalize_kick_url(item.url.trim());
                let live = kick_live(&url).unwrap_or(live);
                let section = item.section.filter(|s| s.is_valid() && !live);
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
                    section,
                    precise: item.precise && section.is_some(),
                    dir: item.dir.unwrap_or_else(|| dir.clone()),
                    stopping: false,
                    raw: None,
                    stem: None,
                    pid: None,
                    parts: 1,
                    parts_done: 0,
                    run: 0,
                    retried: false,
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
            engine::kill(app, pid);
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
            engine::kill(app, pid);
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
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(pid) = job.pid {
                    engine::kill(&app, pid);
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

    /// Deletes a finished download from disk (to the recycle bin on desktop)
    /// and takes it off the list.
    pub fn delete_file(&self, app: &AppHandle, id: u64) -> Result<(), String> {
        let path = {
            let jobs = self.jobs.lock().unwrap();
            let job = jobs
                .iter()
                .find(|j| j.id == id && j.status == Status::Done)
                .ok_or("La descarga ya no está en la lista")?;
            job.path.clone().map(PathBuf::from)
        };
        if let Some(path) = path.filter(|p| p.is_file()) {
            remove_to_trash(&path).map_err(|e| format!("No se pudo borrar el archivo: {e}"))?;
        }
        self.jobs.lock().unwrap().retain(|j| j.id != id);
        let _ = app.emit("job-removed", id);
        Ok(())
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

/// YouTube flags connections that fetch a lot ("confirm you're not a bot", or
/// lately just "This video is not available"). Its embedded and mobile web
/// players still answer then, so every YouTube request asks those too, and for
/// a while after a block requests also go out more slowly.
static YOUTUBE_FALLBACK_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
const YOUTUBE_FALLBACK_FOR: Duration = Duration::from_secs(30 * 60);

pub(crate) fn is_youtube(url: &str) -> bool {
    let u = url.to_ascii_lowercase();
    u.contains("youtube.com/") || u.contains("youtu.be/")
}

/// Extra yt-dlp arguments for YouTube.
pub(crate) fn youtube_args(url: &str) -> Vec<String> {
    if !is_youtube(url) {
        return Vec::new();
    }
    let mut args: Vec<String> = [
        "--extractor-args",
        "youtube:player_client=default,web_embedded,mweb",
    ]
    .map(String::from)
    .to_vec();
    let slow_down = YOUTUBE_FALLBACK_UNTIL
        .lock()
        .unwrap()
        .is_some_and(|until| Instant::now() < until);
    if slow_down {
        args.extend(["--sleep-requests", "1"].map(String::from));
    }
    args
}

/// A block, or the "not available" YouTube now answers with during one.
fn is_youtube_block(error: &str) -> bool {
    error.starts_with("YouTube frenó") || error == NOT_AVAILABLE
}

const NOT_AVAILABLE: &str = "El vídeo no está disponible";

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
            retry_if_blocked(&app, id);
            schedule(&app);
        });
    }
}

/// A YouTube block turns on the fallback players and quietly retries the job
/// once, so the user doesn't have to.
fn retry_if_blocked(app: &AppHandle, id: u64) {
    queue(app).update(app, id, |j| {
        let blocked = j.status == Status::Error
            && j.error.as_deref().is_some_and(is_youtube_block)
            && is_youtube(&j.url);
        if blocked && !j.retried {
            *YOUTUBE_FALLBACK_UNTIL.lock().unwrap() = Some(Instant::now() + YOUTUBE_FALLBACK_FOR);
            j.retried = true;
            j.status = Status::Queued;
            j.error = None;
        }
    });
}

async fn run_job(app: &AppHandle, id: u64, run: u64) -> Result<(), String> {
    let (url, mode, quality, dir, live, section, precise) = {
        let q = queue(app);
        let jobs = q.jobs.lock().unwrap();
        match jobs.iter().find(|j| j.id == id && j.run == run) {
            Some(j) => (
                j.url.clone(),
                j.mode,
                j.quality.clone(),
                j.dir.clone(),
                j.live,
                j.section,
                j.precise,
            ),
            None => return Ok(()),
        }
    };
    // Recording needs FFmpeg, which iOS apps can't run.
    #[cfg(target_os = "ios")]
    if live {
        return Err("En iPhone y iPad todavía no se pueden grabar directos".into());
    }
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|e| e.to_string())?;

    let mut args: Vec<String> = [
        "--ignore-config",
        "--encoding",
        "utf-8",
        "--no-warnings",
        "--newline",
        "--progress",
        "--progress-delta",
        "0.25",
        "--progress-template",
        "download:OVALP|%(progress.status)s|%(progress.downloaded_bytes)s|%(progress.total_bytes)s|%(progress.total_bytes_estimate)s|%(progress.speed)s|%(progress.eta)s",
        "--no-simulate",
        "--print",
        "before_dl:OVALM|%(.{format_id,thumbnail,filename,title})j",
        "--print",
        "after_move:OVALF|%(filepath)j",
        "--no-playlist",
        "--no-mtime",
        "--concurrent-fragments",
        "8",
        // Unstable connections: retry with growing pauses instead of failing,
        // and give Windows antivirus time to let go of a file before renaming.
        "--retries",
        "10",
        "--fragment-retries",
        "10",
        "--retry-sleep",
        "http:exp=1:20",
        "--retry-sleep",
        "fragment:exp=1:20",
        "--file-access-retries",
        "10",
        "--socket-timeout",
        "20",
    ]
    .map(String::from)
    .to_vec();
    args.push("-P".into());
    args.push(format!("home:{}", dir.display()));
    args.push("-P".into());
    args.push(format!("temp:{}", dir.display()));
    if !live {
        // A site's preferred CDN URL may be stale or throttled. Check selected
        // formats and let yt-dlp choose the next downloadable alternative.
        args.push("--check-formats".into());
    }
    // "res:1080" = the best stream no taller than 1080p. H.264/AAC wins ties so
    // files open everywhere, editors included; above 1080p YouTube has no H.264.
    let sort = match &quality {
        Some(h) => format!("res:{h},vcodec:h264,acodec:aac"),
        None => "res,vcodec:h264,acodec:aac".into(),
    };
    let extra: Vec<String> = match mode {
        _ if live => live_args(sort),
        Mode::Video => video_args(sort),
        Mode::Audio => audio_args(quality.as_deref()),
    };
    args.extend(extra);
    if let Some(section) = section {
        clip_args(&mut args, section, precise);
    }
    args.extend(youtube_args(&url));
    // Android's bundled FFmpeg 7.1 refuses YouTube's live audio segments
    // ("extension mismatches allowed extensions") unless told not to be picky.
    #[cfg(target_os = "android")]
    if live {
        args.push("--downloader-args".into());
        args.push("ffmpeg_i:-extension_picky 0".into());
    }
    // iOS: no FFmpeg (merging goes through AVFoundation) and JavaScriptCore
    // instead of an external JavaScript runtime. See engine_plugins/ios.
    #[cfg(target_os = "ios")]
    args.extend(
        [
            "--downloader",
            "native",
            "--fixup",
            "never",
            "--no-js-runtimes",
            "--remote-components",
            "ejs:github",
        ]
        .map(String::from),
    );
    args.push("--".into());
    args.push(url);

    let mut task = engine::ytdlp(app, args).await?;
    let q = queue(app);
    let mut still_ours = false;
    q.update_run(app, id, run, |j| {
        j.pid = Some(task.handle);
        still_ours = true;
    });
    if !still_ours {
        // Paused or canceled while the engine was getting ready.
        engine::kill(app, task.handle);
        return Ok(());
    }

    let finished = Arc::new(AtomicBool::new(false));
    if live {
        tokio::spawn(watch_recording(app.clone(), id, run, finished.clone()));
    }

    let mut error = None;
    let mut last_stderr = None;
    while let Some(line) = task.next_line().await {
        match line {
            Line::Out(line) => handle_line(app, id, run, live, &line),
            Line::Err(line) => {
                if let Some(msg) = line.strip_prefix("ERROR:") {
                    error = Some(clean_error(msg));
                } else if !line.is_empty() && !line.starts_with("WARNING:") {
                    last_stderr = Some(line);
                }
            }
        }
    }
    // Without an "ERROR:" line (Android reports failures its own way), the last
    // thing yt-dlp printed is still more useful than a generic message.
    let error = error.or_else(|| last_stderr.map(|l| clean_error(&l)));
    let success = task.success();
    finished.store(true, Ordering::Relaxed);

    if live {
        return finish_recording(app, id, run, success, error).await;
    }
    let settled = if success {
        let (path, stem) = {
            let jobs = q.jobs.lock().unwrap();
            jobs.iter()
                .find(|j| j.id == id && j.run == run)
                .map(|j| (j.path.clone(), j.stem.clone()))
                .unwrap_or_default()
        };
        let path = find_downloaded(&dir, path.as_deref(), stem.as_deref())
            .ok_or("No se encontró el archivo descargado")?;
        Some(settle_file(&dir, &path)?)
    } else {
        None
    };
    q.update_run(app, id, run, |j| {
        j.pid = None;
        j.speed = 0.0;
        j.eta = None;
        if let Some(p) = settled {
            j.path = Some(p.display().to_string());
        }
        if success {
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

/// Downloads only `section` and names the file after it, so several clips of
/// one video sit next to each other: "Title [id] 15m04s-15m22s.mp4".
fn clip_args(args: &mut Vec<String>, section: Section, precise: bool) {
    if let Some(i) = args.iter().position(|a| a == "-o") {
        if let Some(template) = args.get_mut(i + 1) {
            *template = template.replacen(".%(ext)s", &format!(" {}.%(ext)s", section.label()), 1);
        }
    }
    args.push("--download-sections".into());
    args.push(format!("*{:.3}-{:.3}", section.start, section.end));
    if precise {
        args.push("--force-keyframes-at-cuts".into());
    }
}

/// Recorded as MPEG-TS straight into the final name: a TS file stays playable
/// when the process is cut, and "stop" turns it into an MP4.
fn live_args(sort: String) -> Vec<String> {
    [
        "-o",
        "%(uploader,channel,id).80B %(epoch>%Y-%m-%d %H-%M-%S)s.%(ext)s",
        "-f",
        "b/bv*+ba",
        "-S",
        &sort,
        "--hls-use-mpegts",
        "--no-part",
    ]
    .map(String::from)
    .to_vec()
}

#[cfg(not(target_os = "ios"))]
fn video_args(sort: String) -> Vec<String> {
    [
        "-o",
        "%(title).150B [%(id)s].%(ext)s",
        "-f",
        "bv*+ba/b",
        "-S",
        &sort,
        "--merge-output-format",
        "mp4",
        // Single-file downloads (archive.org, some sites) come as AVI,
        // FLV or MOV; repackage them so everything ends up as MP4.
        "--remux-video",
        "avi>mp4/flv>mp4/mov>mp4/webm>mp4",
    ]
    .map(String::from)
    .to_vec()
}

/// AVFoundation joins H.264 + AAC into MP4 without re-encoding, so iOS asks
/// for those (YouTube has them up to 1080p) and keeps other single files as
/// they come.
#[cfg(target_os = "ios")]
fn video_args(sort: String) -> Vec<String> {
    [
        "-o",
        "%(title).150B [%(id)s].%(ext)s",
        "-f",
        "bv*[vcodec^=avc1]+ba[ext=m4a]/b[ext=mp4]/bv*[ext=mp4]+ba[ext=m4a]/b",
        "-S",
        &sort,
        "--merge-output-format",
        "mp4",
    ]
    .map(String::from)
    .to_vec()
}

#[cfg(not(target_os = "ios"))]
fn audio_args(bitrate: Option<&str>) -> Vec<String> {
    let bitrate = bitrate.map_or_else(|| "0".into(), |k| format!("{k}K"));
    [
        "-o",
        "%(title).150B [%(id)s].%(ext)s",
        "-f",
        "ba/b",
        "-x",
        "--audio-format",
        "mp3",
        "--audio-quality",
        &bitrate,
    ]
    .map(String::from)
    .to_vec()
}

/// No MP3 encoder on iOS: the AAC (M4A) audio is saved as it comes.
#[cfg(target_os = "ios")]
fn audio_args(_bitrate: Option<&str>) -> Vec<String> {
    [
        "-o",
        "%(title).150B [%(id)s].%(ext)s",
        "-f",
        "ba[ext=m4a]/ba/b",
    ]
    .map(String::from)
    .to_vec()
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
    id: u64,
    run: u64,
    success: bool,
    error: Option<String>,
) -> Result<(), String> {
    let (raw, stopping, dir) = {
        let q = queue(app);
        let jobs = q.jobs.lock().unwrap();
        match jobs.iter().find(|j| j.id == id && j.run == run) {
            Some(j) => (
                j.path
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|p| p.is_file())
                    .or_else(|| j.raw.clone()),
                j.stopping,
                j.dir.clone(),
            ),
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
        finalize_recording(app, &raw).await
    };
    let path = settle_file(&dir, &path)?;
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
#[cfg(desktop)]
async fn finalize_recording(app: &AppHandle, raw: &Path) -> PathBuf {
    let ts = raw.with_extension("ts");
    if std::fs::rename(raw, &ts).is_err() {
        return raw.to_path_buf();
    }
    let Ok(tools) = crate::tools::ensure(app).await else {
        return ts;
    };
    let mp4 = raw.with_extension("mp4");
    let mut cmd = tokio::process::Command::new(tools.ffmpeg());
    cmd.args(["-y", "-loglevel", "error", "-i"])
        .arg(&ts)
        .args(["-map", "0", "-c", "copy", "-movflags", "+faststart"])
        .arg(&mp4)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    crate::platform::prepare(&mut cmd);
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

/// On Android the recording stays as MPEG-TS, which phone players open fine.
#[cfg(mobile)]
async fn finalize_recording(_app: &AppHandle, raw: &Path) -> PathBuf {
    let ts = raw.with_extension("ts");
    match std::fs::rename(raw, &ts) {
        Ok(()) => ts,
        Err(_) => raw.to_path_buf(),
    }
}

/// ASCII JSON on the wire preserves Windows filenames independently of the
/// engine's console encoding, including accents, emoji, quotes and delimiters.
#[derive(Deserialize)]
struct DownloadMetadata {
    format_id: Option<String>,
    thumbnail: Option<String>,
    filename: Option<String>,
    title: Option<String>,
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
        let Ok(metadata) = serde_json::from_str::<DownloadMetadata>(rest) else {
            return;
        };
        let format = metadata.format_id.as_deref().unwrap_or("");
        let thumb = metadata.thumbnail.as_deref().unwrap_or("");
        let file = metadata.filename.as_deref().unwrap_or("");
        let title = metadata.title.as_deref().unwrap_or("");
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
                let path = PathBuf::from(file);
                j.raw = Some(if path.is_absolute() {
                    path
                } else {
                    j.dir.join(path)
                });
            }
            if j.status == Status::Starting {
                j.status = Status::Downloading;
            }
        });
    } else if let Some(path) = line.strip_prefix("OVALF|") {
        if let Ok(path) = serde_json::from_str::<String>(path) {
            q.update_run(app, id, run, |j| j.path = Some(path));
        }
    }
}

/// The finished file. yt-dlp's final "after_move" line can go missing (the
/// Android engine may report the exit before its last output) or name a path
/// that does not match the disk, so look for the file by name before giving up:
/// "Title [id].mp4" in the job folder or already in its own subfolder.
fn find_downloaded(dir: &Path, reported: Option<&str>, stem: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = reported.filter(|p| !p.is_empty()) {
        let p = Path::new(p);
        let p = if p.is_absolute() {
            p.to_path_buf()
        } else {
            dir.join(p)
        };
        if p.is_file() {
            return Some(p);
        }
    }
    let stem = reported
        .and_then(|p| Path::new(p).file_stem())
        .map(|s| s.to_string_lossy().into_owned())
        .into_iter()
        .chain(stem.map(String::from))
        .filter(|s| !s.is_empty());
    for stem in stem {
        for folder in [dir.join(&stem), dir.to_path_buf()] {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            let found = entries.flatten().map(|e| e.path()).find(|p| {
                p.is_file()
                    && p.file_stem().is_some_and(|s| s.to_string_lossy() == stem)
                    && p.extension()
                        .is_some_and(|e| is_media(&e.to_string_lossy()))
            });
            if found.is_some() {
                return found;
            }
        }
    }
    None
}

/// Single links and recordings stay in the selected root. Channel jobs already
/// have their channel subfolder in `dir`. Never create a per-video folder.
fn settle_file(dir: &Path, path: &Path) -> Result<PathBuf, String> {
    let name = path.file_name().ok_or("El archivo no tiene nombre")?;
    let folder = dir.to_path_buf();
    let target = folder.join(name);
    if !path.is_file() {
        return Err("No se encontró el archivo descargado".into());
    }
    if same_dir(path.parent(), &folder) {
        return Ok(path.to_path_buf());
    }
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("No se pudo crear la carpeta del vídeo: {e}"))?;
    if target.exists() {
        return Err(format!(
            "Ya existe otro archivo en {}. La descarga se conserva en {}",
            target.display(),
            path.display()
        ));
    }
    // A hard link moves across folders without copying gigabytes and cannot
    // overwrite a destination. Fall back to an exclusive copy across disks.
    if std::fs::hard_link(path, &target).is_err() {
        let mut source = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| e.to_string())?;
        if let Err(e) = std::io::copy(&mut source, &mut output).and_then(|_| output.sync_all()) {
            drop(output);
            let _ = std::fs::remove_file(&target);
            return Err(format!("No se pudo colocar el vídeo en su carpeta: {e}"));
        }
    }
    std::fs::remove_file(path)
        .map_err(|e| format!("El vídeo se copió, pero no se pudo retirar el original: {e}"))?;
    if !target.is_file() || !same_dir(target.parent(), &folder) {
        return Err("No se pudo validar la carpeta del vídeo".into());
    }
    Ok(target)
}

fn same_dir(a: Option<&Path>, b: &Path) -> bool {
    let Some(a) = a else { return false };
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
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
    } else if lower.contains("not a bot")
        || lower.contains("too many requests")
        || lower.contains("page needs to be reloaded")
    {
        // YouTube throttles connections that fetch a lot in a short time.
        "YouTube frenó la conexión por exceso de descargas; reintenta en unos minutos".into()
    } else if lower.contains("private") {
        "El vídeo es privado".into()
    } else if lower.contains("sign in") || lower.contains("login") || lower.contains("cookies") {
        "El sitio pide iniciar sesión".into()
    } else if lower.contains("unavailable") || lower.contains("not available") {
        NOT_AVAILABLE.into()
    } else {
        msg.chars().take(140).collect()
    }
}

/// Kick's old shared /video/UUID links still circulate; the current extractor
/// uses /channel/videos/UUID and fetches metadata by UUID alone.
fn normalize_kick_url(url: &str) -> String {
    let Some(rest) = kick_path(url) else {
        return url.to_string();
    };
    if let Some(id) = rest.strip_prefix("video/") {
        let id = id.split(['?', '#', '/']).next().unwrap_or("");
        if id.len() == 36 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
            return format!("https://kick.com/_/videos/{id}");
        }
    }
    url.to_string()
}

fn kick_path(url: &str) -> Option<&str> {
    let (_, rest) = url.split_once("://")?;
    let (host, path) = rest.split_once('/')?;
    (host.eq_ignore_ascii_case("kick.com") || host.eq_ignore_ascii_case("www.kick.com"))
        .then_some(path)
}

/// Recorded videos and clips stay ordinary downloads even in the live tab.
fn kick_live(url: &str) -> Option<bool> {
    let path = kick_path(url)?;
    let channel = path.split(['?', '#']).next()?.trim_end_matches('/');
    Some(
        !channel.is_empty()
            && !channel.contains('/')
            && !url.contains("clip=")
            && !matches!(
                channel,
                "categories" | "search" | "auth" | "videos" | "clips"
            ),
    )
}

fn short_url(url: &str) -> String {
    let s = url
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.");
    s.chars().take(60).collect()
}

/// What is already in the download folder (and the channel's subfolder).
#[derive(Debug, Default, Serialize)]
pub struct OnDisk {
    /// Finished files: Oval names them "Title [id].ext".
    pub ids: Vec<String>,
    /// Downloads cut halfway (".part" files): yt-dlp continues them.
    pub partial: Vec<String>,
    /// Media files without an id, matched by title instead.
    pub names: Vec<String>,
}

pub fn on_disk(app: &AppHandle, folder: Option<&str>) -> OnDisk {
    let root = download_root(app);
    let mut dirs = vec![root.clone()];
    if let Some(f) = folder.map(sanitize).filter(|f| !f.is_empty()) {
        dirs.push(root.join(f));
    }
    let mut found = OnDisk::default();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let Ok(kind) = e.file_type() else { continue };
            let files = if kind.is_dir() {
                std::fs::read_dir(e.path())
                    .map(|items| items.flatten().collect::<Vec<_>>())
                    .unwrap_or_default()
            } else {
                vec![e]
            };
            for e in files {
                if !e.file_type().is_ok_and(|kind| kind.is_file()) {
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                match classify(&name) {
                    Some(File::Done(id)) => found.ids.push(id.to_string()),
                    Some(File::Partial(id)) => found.partial.push(id.to_string()),
                    Some(File::Named(title)) => found.names.push(title.to_string()),
                    None => {}
                }
            }
        }
    }
    for list in [&mut found.ids, &mut found.partial, &mut found.names] {
        list.sort();
        list.dedup();
    }
    found
}

#[derive(Debug, PartialEq)]
enum File<'a> {
    Done(&'a str),
    Partial(&'a str),
    Named(&'a str),
}

const MEDIA: &[&str] = &[
    "mp4", "mkv", "webm", "mp3", "m4a", "opus", "avi", "mov", "ts", "flv",
];

fn is_media(ext: &str) -> bool {
    MEDIA.contains(&ext.to_ascii_lowercase().as_str())
}

/// "Title [id].mp4" is done; "Title [id].f137.mp4.part", "Title [id].mp4.ytdl"
/// or a lone "Title [id].f251.webm" stream is a download left halfway.
fn classify(name: &str) -> Option<File<'_>> {
    let bracket = name.rfind('[').and_then(|open| {
        let close = open + name[open..].find(']')?;
        let id = &name[open + 1..close];
        let valid = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        valid.then_some((id, &name[close + 1..]))
    });
    if let Some((id, rest)) = bracket {
        let exts: Vec<&str> = rest.strip_prefix('.')?.split('.').collect();
        return match exts.as_slice() {
            [ext] if is_media(ext) => Some(File::Done(id)),
            [.., "part" | "ytdl"] => Some(File::Partial(id)),
            [stream, ext]
                if is_media(ext)
                    && (*stream == "temp"
                        || stream.strip_prefix('f').is_some_and(|n| {
                            !n.is_empty()
                                && n.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                        })) =>
            {
                Some(File::Partial(id))
            }
            _ => None,
        };
    }
    let (stem, ext) = name.rsplit_once('.')?;
    (is_media(ext) && !stem.trim().is_empty()).then_some(File::Named(stem.trim()))
}

#[cfg(desktop)]
fn remove_to_trash(path: &Path) -> Result<(), String> {
    trash::delete(path)
        .or_else(|_| std::fs::remove_file(path))
        .map_err(|e| e.to_string())
}

#[cfg(mobile)]
fn remove_to_trash(path: &Path) -> Result<(), String> {
    std::fs::remove_file(path).map_err(|e| e.to_string())
}

/// A folder name that is valid everywhere, Windows included: no reserved
/// characters or device names, and no trailing dots or spaces (which Windows
/// drops, so the folder Oval asks for and the one it gets would differ).
pub fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if r#"<>:"/\|?*"#.contains(c) || c.is_control() {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let short: String = cleaned.chars().take(80).collect();
    let mut out = short.trim_end_matches(['.', ' ']).to_string();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem.len() == 4
            && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.push('_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kick_links_choose_the_right_mode() {
        let vod = "https://kick.com/facuebbes/videos/01a11bec-c058-7ab7-9ad2-1071ac6a5bbb";
        assert_eq!(kick_live(vod), Some(false));
        assert_eq!(
            kick_live("https://www.kick.com/facuebbes/?foo=1"),
            Some(true)
        );
        assert_eq!(
            kick_live("https://kick.com/facuebbes?clip=clip_123"),
            Some(false)
        );
        assert_eq!(
            kick_live("https://kick.com/facuebbes/clips/clip_123"),
            Some(false)
        );
        assert_eq!(kick_live("https://kick.com.evil/facuebbes"), None);
        assert_eq!(
            normalize_kick_url("https://kick.com/video/01a11bec-c058-7ab7-9ad2-1071ac6a5bbb"),
            "https://kick.com/_/videos/01a11bec-c058-7ab7-9ad2-1071ac6a5bbb"
        );
    }

    #[test]
    fn clips_are_named_after_their_range() {
        let s = Section {
            start: 904.8,
            end: 922.2,
        };
        assert_eq!(s.label(), "15m04.8s-15m22.2s");
        assert_eq!(
            Section {
                start: 3600.0,
                end: 3661.0
            }
            .label(),
            "1h00m00s-1h01m01s"
        );
        assert!(!Section {
            start: 5.0,
            end: 5.0
        }
        .is_valid());
        let mut args: Vec<String> = ["-o", "%(title).150B [%(id)s].%(ext)s"]
            .map(String::from)
            .to_vec();
        clip_args(&mut args, s, true);
        assert_eq!(args[1], "%(title).150B [%(id)s] 15m04.8s-15m22.2s.%(ext)s");
        assert_eq!(args[3], "*904.800-922.200");
        assert_eq!(args[4], "--force-keyframes-at-cuts");
    }

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
        assert!(
            clean_error("[youtube] x: Sign in to confirm you’re not a bot. Use --cookies")
                .starts_with("YouTube frenó")
        );
        assert_eq!(
            clean_error("ERROR: [youtube] Y5N_KNDfAls: This video is not available"),
            NOT_AVAILABLE
        );
        assert!(is_youtube_block(&clean_error(
            "[youtube] x: This video is not available"
        )));
    }

    #[test]
    fn files_on_disk_are_classified() {
        assert_eq!(
            classify("Coffee Run [PVGeM40dABA].mp4"),
            Some(File::Done("PVGeM40dABA"))
        );
        assert_eq!(classify("a [b] c [x_y-1].mp3"), Some(File::Done("x_y-1")));
        assert_eq!(
            classify("Clip [abc].f137.mp4.part"),
            Some(File::Partial("abc"))
        );
        assert_eq!(classify("Clip [abc].mp4.ytdl"), Some(File::Partial("abc")));
        assert_eq!(classify("Clip [abc].f251.webm"), Some(File::Partial("abc")));
        assert_eq!(classify("Clip [abc].webp"), None);
        assert_eq!(classify("notes [abc].txt"), None);
        assert_eq!(classify("Old clip.mp4"), Some(File::Named("Old clip")));
        assert_eq!(classify("cover.jpg"), None);
        assert!(is_youtube("https://youtu.be/x") && !is_youtube("https://vimeo.com/1"));
    }

    #[test]
    fn folder_names_are_safe() {
        assert_eq!(sanitize("A/B: C?"), "A B C");
        assert_eq!(sanitize("name."), "name");
        assert_eq!(sanitize("  Canal   de prueba . "), "Canal de prueba");
        assert_eq!(sanitize("con"), "con_");
        assert_eq!(sanitize("COM1.txt"), "COM1.txt_");
        let long = format!("{} . x", "a".repeat(78));
        assert!(!sanitize(&long).ends_with([' ', '.']));
    }

    #[test]
    fn json_metadata_preserves_unicode_and_delimiters() {
        let name = "Nature’s 世界 | quote \"x\" [id].mp4";
        let raw = serde_json::json!({"filename": name, "title": "title | caption", "format_id": "v+a", "thumbnail": null}).to_string();
        let metadata: DownloadMetadata = serde_json::from_str(&raw).unwrap();
        assert_eq!(metadata.filename.as_deref(), Some(name));
        assert_eq!(metadata.title.as_deref(), Some("title | caption"));
        let path = serde_json::to_string(&name).unwrap();
        assert_eq!(serde_json::from_str::<String>(&path).unwrap(), name);
    }

    #[test]
    fn individual_downloads_do_not_create_folders() {
        let dir = std::env::temp_dir().join(format!("oval-flat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Nature’s gifts [id].mp4");
        std::fs::write(&file, b"video").unwrap();
        assert_eq!(settle_file(&dir, &file).unwrap(), file);
        assert!(!dir.join("Nature’s gifts [id]").exists());
        assert_eq!(
            find_downloaded(&dir, None, Some("Nature’s gifts [id]")),
            Some(file)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn downloads_are_found_without_their_final_path() {
        let dir = std::env::temp_dir().join(format!("oval-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("Video [x].mp4");
        std::fs::write(&file, b"v").unwrap();
        std::fs::write(dir.join("Video [x].f137.mp4.part"), b"p").unwrap();
        // The final path line never arrived: the name from before_dl is enough,
        // even when the remux changed the extension.
        assert_eq!(
            find_downloaded(&dir, None, Some("Video [x]")),
            Some(file.clone())
        );
        let missing = dir.join("otro/Video [x].mp4").display().to_string();
        assert_eq!(
            find_downloaded(&dir, Some(&missing), None),
            Some(file.clone())
        );
        let settled = settle_file(&dir, &file).unwrap();
        assert_eq!(
            find_downloaded(&dir, None, Some("Video [x]")),
            Some(settled)
        );
        assert_eq!(find_downloaded(&dir, None, Some("Nada")), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn stray_files_land_in_their_folder() {
        let root = std::env::temp_dir().join(format!("oval-settle-{}", std::process::id()));
        let dir = root.join("Canal");
        std::fs::create_dir_all(dir.join("Video [x]")).unwrap();
        std::fs::create_dir_all(root.join("Video [x]")).unwrap();
        let stray = root.join("Video [x].mp4");
        std::fs::write(&stray, b"v").unwrap();
        let settled = settle_file(&dir, &stray).unwrap();
        assert_eq!(settled, dir.join("Video [x].mp4"));
        assert!(!stray.exists() && settled.is_file());
        assert!(dir.join("Video [x]").is_dir());
        // Already in place: left alone.
        assert_eq!(settle_file(&dir, &settled).unwrap(), settled);
        std::fs::write(&stray, b"new").unwrap();
        assert!(settle_file(&dir, &stray).is_err());
        assert_eq!(std::fs::read(&settled).unwrap(), b"v");
        assert_eq!(std::fs::read(&stray).unwrap(), b"new");
        std::fs::remove_dir_all(&root).unwrap();
    }
}

//! Runs yt-dlp. On desktop it is a child process (see `tools` for where the
//! binaries come from); on Android and iOS it goes through the embedded engine
//! of the `oval-engine` plugin. Callers see the same thing either way: a stream of
//! output lines followed by an exit status.

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::mpsc;

#[derive(Clone, Serialize)]
pub struct EngineStatus {
    pub ready: bool,
    pub message: String,
    pub progress: Option<f64>,
}

static LAST: std::sync::Mutex<Option<EngineStatus>> = std::sync::Mutex::new(None);

/// The latest engine status, for a UI that subscribed after it was emitted.
pub fn status() -> EngineStatus {
    LAST.lock().unwrap().clone().unwrap_or(EngineStatus {
        ready: false,
        message: "Preparando motor".into(),
        progress: None,
    })
}

pub fn emit(app: &AppHandle, ready: bool, message: &str, progress: Option<f64>) {
    let status = EngineStatus {
        ready,
        message: message.into(),
        progress,
    };
    *LAST.lock().unwrap() = Some(status.clone());
    let _ = app.emit("engine", status);
}

pub enum Line {
    Out(String),
    Err(String),
}

enum Msg {
    Line(Line),
    Exit(bool),
}

/// Identifies a running yt-dlp so it can be stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(u64);

pub struct Task {
    pub handle: Handle,
    rx: mpsc::UnboundedReceiver<Msg>,
    success: bool,
}

impl Task {
    /// The next output line, or `None` once yt-dlp has exited.
    pub async fn next_line(&mut self) -> Option<Line> {
        match self.rx.recv().await {
            Some(Msg::Line(line)) => Some(line),
            Some(Msg::Exit(ok)) => {
                self.success = ok;
                None
            }
            None => None,
        }
    }

    /// Whether yt-dlp exited cleanly; meaningful after `next_line` returned `None`.
    pub fn success(&self) -> bool {
        self.success
    }
}

/// Gets the engine ready (downloads it on desktop, unpacks it on Android) and
/// then keeps yt-dlp current in the background.
pub async fn warm_up(app: AppHandle) {
    if prepare(&app).await.is_ok() {
        update(app).await;
    }
}

// ---------------------------------------------------------------- desktop

/// Embedded with the app so channel support ships in the portable executable
/// and uses the same extractor on desktop and Android.
async fn channel_plugins(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    static PREPARED_PLUGINS: tokio::sync::Mutex<Option<std::path::PathBuf>> =
        tokio::sync::Mutex::const_new(None);
    let mut prepared = PREPARED_PLUGINS.lock().await;
    if let Some(path) = prepared.as_ref() {
        return Ok(path.clone());
    }
    let root = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("engine-plugins")
        .join(env!("CARGO_PKG_VERSION"));
    let extractor = root.join("oval/yt_dlp_plugins/extractor");
    tokio::fs::create_dir_all(&extractor)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::write(
        extractor.join("oval_kick.py"),
        include_str!("../engine_plugins/oval/yt_dlp_plugins/extractor/oval_kick.py"),
    )
    .await
    .map_err(|e| e.to_string())?;
    *prepared = Some(root.clone());
    Ok(root)
}

#[cfg(desktop)]
pub async fn prepare(app: &AppHandle) -> Result<(), String> {
    crate::tools::ensure(app).await.map(|_| ())
}

#[cfg(desktop)]
async fn update(app: AppHandle) {
    crate::tools::update_in_background(app).await;
}

#[cfg(desktop)]
pub async fn ytdlp(app: &AppHandle, args: Vec<String>) -> Result<Task, String> {
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, BufReader};

    let tools = crate::tools::ensure(app).await?;
    let plugins = channel_plugins(app).await?;
    let mut cmd = tokio::process::Command::new(&tools.ytdlp);
    cmd.arg("--plugin-dirs").arg(plugins);
    if let Some(ff) = &tools.ffmpeg_dir {
        cmd.arg("--ffmpeg-location").arg(ff);
    }
    if let Some(deno) = &tools.deno {
        cmd.arg("--js-runtimes")
            .arg(format!("deno:{}", deno.display()));
    }
    cmd.args(&args)
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    crate::platform::prepare(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("No se pudo iniciar yt-dlp: {e}"))?;
    crate::platform::adopt_child(&child);
    let handle = Handle(child.id().unwrap_or_default() as u64);

    let (tx, rx) = mpsc::unbounded_channel();
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let out_tx = tx.clone();
    let out = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).split(b'\n');
        while let Ok(Some(raw)) = lines.next_segment().await {
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            let _ = out_tx.send(Msg::Line(Line::Out(line)));
        }
    });
    let err_tx = tx.clone();
    let err = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).split(b'\n');
        while let Ok(Some(raw)) = lines.next_segment().await {
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            let _ = err_tx.send(Msg::Line(Line::Err(line)));
        }
    });
    tokio::spawn(async move {
        let _ = tokio::join!(out, err);
        let ok = child.wait().await.is_ok_and(|s| s.success());
        let _ = tx.send(Msg::Exit(ok));
    });
    Ok(Task {
        handle,
        rx,
        success: false,
    })
}

#[cfg(desktop)]
pub fn kill(_app: &AppHandle, handle: Handle) {
    if handle.0 != 0 {
        crate::platform::kill_tree(handle.0 as u32);
    }
}

// ---------------------------------------------------------------- mobile

#[cfg(mobile)]
static PREPARED: tokio::sync::Mutex<bool> = tokio::sync::Mutex::const_new(false);

#[cfg(mobile)]
fn engine(app: &AppHandle) -> tauri::State<'_, tauri_plugin_oval_engine::Engine<tauri::Wry>> {
    app.state::<tauri_plugin_oval_engine::Engine<tauri::Wry>>()
}

/// The iOS engine runs `engine_plugins/ios/oval_ios.py` on its embedded
/// Python and keeps yt-dlp updates in the app's data folder.
#[cfg(target_os = "ios")]
fn setup(app: &AppHandle) -> Result<tauri_plugin_oval_engine::Setup, String> {
    let driver = app
        .path()
        .app_cache_dir()
        .map_err(|e| e.to_string())?
        .join("engine-ios")
        .join(env!("CARGO_PKG_VERSION"));
    std::fs::create_dir_all(&driver).map_err(|e| e.to_string())?;
    std::fs::write(
        driver.join("oval_ios.py"),
        include_str!("../engine_plugins/ios/oval_ios.py"),
    )
    .map_err(|e| e.to_string())?;
    let home = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&home).map_err(|e| e.to_string())?;
    Ok(tauri_plugin_oval_engine::Setup {
        driver: Some(driver.display().to_string()),
        home: Some(home.display().to_string()),
    })
}

#[cfg(target_os = "android")]
fn setup(_app: &AppHandle) -> Result<tauri_plugin_oval_engine::Setup, String> {
    Ok(tauri_plugin_oval_engine::Setup::default())
}

#[cfg(mobile)]
pub async fn prepare(app: &AppHandle) -> Result<(), String> {
    let mut done = PREPARED.lock().await;
    if *done {
        return Ok(());
    }
    emit(app, false, "Preparando motor", None);
    let handle = app.clone();
    let setup = setup(app)?;
    let result = tokio::task::spawn_blocking(move || engine(&handle).prepare(setup))
        .await
        .map_err(|e| e.to_string())?;
    match result {
        Ok(_) => {
            *done = true;
            emit(app, true, "Listo", None);
            Ok(())
        }
        Err(e) => {
            emit(app, false, "No se pudo preparar el motor", None);
            Err(e)
        }
    }
}

#[cfg(mobile)]
async fn update(app: AppHandle) {
    let _ = tokio::task::spawn_blocking(move || engine(&app).update()).await;
}

#[cfg(mobile)]
pub async fn ytdlp(app: &AppHandle, mut args: Vec<String>) -> Result<Task, String> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);

    prepare(app).await?;
    let plugins = channel_plugins(app).await?;
    args.splice(
        0..0,
        ["--plugin-dirs".into(), plugins.display().to_string()],
    );
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let (tx, rx) = mpsc::unbounded_channel();
    let handle = app.clone();
    tokio::task::spawn_blocking(move || {
        engine(&handle).run(id, args, move |event| {
            if let Some(line) = event.out {
                let _ = tx.send(Msg::Line(Line::Out(line.trim().to_string())));
            }
            if let Some(line) = event.err {
                let _ = tx.send(Msg::Line(Line::Err(line.trim().to_string())));
            }
            if let Some(code) = event.exit {
                let _ = tx.send(Msg::Exit(code == 0));
            }
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Task {
        handle: Handle(id),
        rx,
        success: false,
    })
}

#[cfg(mobile)]
pub fn kill(app: &AppHandle, handle: Handle) {
    let app = app.clone();
    std::thread::spawn(move || engine(&app).kill(handle.0));
}

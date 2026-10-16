//! Oval: the same code runs the desktop app (`main.rs`) and the Android and
//! iOS apps.

#[cfg(desktop)]
mod bridge;
mod channel;
mod downloads;
mod engine;
#[cfg(desktop)]
mod extras;
#[cfg(desktop)]
mod live;
#[cfg(desktop)]
mod platform;
mod settings;
#[cfg(desktop)]
mod tools;
mod transcript;

use downloads::{Job, Mode, NewJob, Queue};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_clipboard_manager::ClipboardExt;

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command]
fn jobs(queue: State<Queue>) -> Vec<Job> {
    queue.list()
}

#[tauri::command]
fn add_download(
    app: AppHandle,
    queue: State<Queue>,
    url: String,
    mode: Mode,
    quality: Option<String>,
) -> Vec<Job> {
    let item = NewJob {
        url,
        ..NewJob::default()
    };
    queue.add(&app, vec![item], mode, quality.as_deref(), None, false)
}

#[tauri::command]
fn add_live(app: AppHandle, queue: State<Queue>, url: String, quality: Option<String>) -> Vec<Job> {
    let item = NewJob {
        url,
        ..NewJob::default()
    };
    queue.add(
        &app,
        vec![item],
        Mode::Video,
        quality.as_deref(),
        None,
        true,
    )
}

#[tauri::command]
fn stop_live(app: AppHandle, queue: State<Queue>, id: u64) {
    queue.stop_live(&app, id)
}

#[tauri::command]
fn add_many(
    app: AppHandle,
    queue: State<Queue>,
    items: Vec<NewJob>,
    mode: Mode,
    quality: Option<String>,
    folder: Option<String>,
) -> Vec<Job> {
    queue.add(
        &app,
        items,
        mode,
        quality.as_deref(),
        folder.as_deref(),
        false,
    )
}

#[tauri::command]
fn pause(app: AppHandle, queue: State<Queue>, id: u64) {
    queue.pause(&app, id)
}

#[tauri::command]
fn resume(app: AppHandle, queue: State<Queue>, id: u64) {
    queue.resume(&app, id)
}

#[tauri::command]
fn cancel(app: AppHandle, queue: State<Queue>, id: u64) {
    queue.cancel(&app, id)
}

#[tauri::command]
fn delete_file(app: AppHandle, queue: State<Queue>, id: u64) -> Result<(), String> {
    queue.delete_file(&app, id)
}

#[tauri::command]
fn clear_finished(app: AppHandle, queue: State<Queue>) {
    queue.clear_finished(&app)
}

#[tauri::command]
fn reveal(app: AppHandle, queue: State<Queue>, id: Option<u64>) {
    let path = id
        .and_then(|id| queue.path_of(id))
        .unwrap_or_else(|| downloads::download_root(&app));
    if !path.exists() {
        let _ = std::fs::create_dir_all(&path);
    }
    #[cfg(desktop)]
    platform::reveal(&path);
}

#[tauri::command]
fn get_concurrency(app: AppHandle) -> u32 {
    settings::get(&app).concurrency
}

#[tauri::command]
fn set_concurrency(app: AppHandle, value: u32) -> u32 {
    let s = settings::update(&app, |s| s.concurrency = value.clamp(1, 8));
    downloads::schedule(&app);
    s.concurrency
}

#[tauri::command]
async fn list_channel(
    app: AppHandle,
    url: String,
    tab: String,
    token: u64,
) -> Result<channel::Summary, String> {
    channel::list(&app, &url, &tab, token).await
}

#[tauri::command]
fn cancel_listing(app: AppHandle) {
    channel::cancel(&app)
}

#[tauri::command]
fn clipboard_text(app: AppHandle) -> Option<String> {
    app.clipboard().read_text().ok()
}

#[tauri::command]
fn on_disk(app: AppHandle, folder: Option<String>) -> downloads::OnDisk {
    downloads::on_disk(&app, folder.as_deref())
}

#[tauri::command]
fn engine_status() -> engine::EngineStatus {
    engine::status()
}

/// "windows", "linux" or "android", so the UI can adapt.
#[tauri::command]
fn platform() -> &'static str {
    std::env::consts::OS
}

#[tauri::command]
fn download_folder(app: AppHandle) -> String {
    downloads::download_root(&app).display().to_string()
}

/// Lets the user pick where downloads go; `None` if the dialog was dismissed.
#[cfg(desktop)]
#[tauri::command]
async fn choose_folder(app: AppHandle) -> Option<String> {
    let start = Some(downloads::download_root(&app));
    let path = pick_folder(&app, "Carpeta de descargas", start).await?;
    settings::update(&app, |s| s.folder = Some(path.clone()));
    Some(path.display().to_string())
}

/// A native folder picker on top of Oval's window.
#[cfg(desktop)]
pub(crate) async fn pick_folder(
    app: &AppHandle,
    title: &str,
    start: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app.dialog().file().set_title(title);
    if let Some(start) = start.filter(|p| p.is_dir()) {
        dialog = dialog.set_directory(start);
    }
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.set_focus();
        dialog = dialog.set_parent(&w);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    rx.await.ok()??.into_path().ok()
}

/// Phones keep downloads in Download/Oval; there is no folder picker.
#[cfg(mobile)]
#[tauri::command]
async fn choose_folder() -> Option<String> {
    None
}

/// Smoke test for phones and simulators in CI: with `OVAL_SELFTEST=<link>`
/// the app downloads the link, reports the outcome and quits. iOS doesn't
/// hand stdout to the simulator console, so each line also goes to
/// `selftest.log` in the app's cache folder.
#[cfg(mobile)]
async fn selftest(app: AppHandle, url: String) {
    use downloads::Status;
    use std::io::Write;

    let log = app.path().app_cache_dir().ok().map(|d| {
        let _ = std::fs::create_dir_all(&d);
        d.join("selftest.log")
    });
    let report = |line: String| {
        println!("{line}");
        if let Some(file) = log.as_ref().and_then(|p| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(p)
                .ok()
        }) {
            let _ = writeln!(&file, "{line}");
        }
    };
    report("OVAL_SELFTEST_STEP started".into());
    let queue = app.state::<Queue>();
    let item = NewJob {
        url,
        ..NewJob::default()
    };
    let id = queue.add(&app, vec![item], Mode::Video, Some("360"), None, false)[0].id;
    for second in 0..900 {
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        let Some(job) = queue.list().into_iter().find(|j| j.id == id) else {
            break;
        };
        if second % 15 == 0 {
            report(format!(
                "OVAL_SELFTEST_STEP {:?} {:.0}% engine: {}",
                job.status,
                job.progress * 100.0,
                engine::status().message
            ));
        }
        match job.status {
            Status::Done => {
                report(format!("OVAL_SELFTEST ok {}", job.path.unwrap_or_default()));
                std::process::exit(0);
            }
            Status::Error => {
                report(format!(
                    "OVAL_SELFTEST error {}",
                    job.error.unwrap_or_default()
                ));
                std::process::exit(1);
            }
            _ => {}
        }
    }
    report("OVAL_SELFTEST timeout".into());
    std::process::exit(1);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    // One Oval at a time: starting it again (or opening an oval:// link, as
    // the browser extension does) brings the running one to the front.
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            platform::bring_to_front(app)
        }))
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_dialog::init());
    let builder = builder.plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_oval_engine::init());
    builder
        .manage(Queue::default())
        .setup(|app| {
            let settings = settings::load(app.handle());
            app.manage(settings::Store(std::sync::Mutex::new(settings)));
            #[cfg(desktop)]
            if let Some(window) = app.get_webview_window("main") {
                platform::click_through_outside_oval(window);
            }
            #[cfg(desktop)]
            bridge::start(app.handle().clone());
            // oval:// opens this copy of Oval, wherever it lives (installed or
            // portable): registered again on every start so it follows moves.
            #[cfg(any(windows, target_os = "linux"))]
            {
                use tauri_plugin_deep_link::DeepLinkExt;
                if let Err(e) = app.deep_link().register("oval") {
                    eprintln!("Oval: oval:// link not registered: {e}");
                }
            }
            #[cfg(windows)]
            {
                let handle = app.handle().clone();
                platform::keep_awake_while(move || handle.state::<Queue>().busy());
            }
            // Warm the engine up front so the first download starts instantly.
            tauri::async_runtime::spawn(engine::warm_up(app.handle().clone()));
            #[cfg(mobile)]
            if let Ok(url) = std::env::var("OVAL_SELFTEST") {
                tauri::async_runtime::spawn(selftest(app.handle().clone(), url));
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            jobs,
            add_download,
            add_many,
            add_live,
            stop_live,
            pause,
            resume,
            cancel,
            clear_finished,
            delete_file,
            reveal,
            get_concurrency,
            set_concurrency,
            list_channel,
            cancel_listing,
            clipboard_text,
            engine_status,
            on_disk,
            platform,
            download_folder,
            choose_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oval");
}

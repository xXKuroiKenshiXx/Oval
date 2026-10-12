//! Oval: the same code runs the desktop app (`main.rs`) and the Android app.

mod channel;
mod downloads;
mod engine;
#[cfg(desktop)]
mod platform;
mod settings;
#[cfg(desktop)]
mod tools;

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
        title: None,
        thumbnail: None,
    };
    queue.add(&app, vec![item], mode, quality.as_deref(), None, false)
}

#[tauri::command]
fn add_live(app: AppHandle, queue: State<Queue>, url: String, quality: Option<String>) -> Vec<Job> {
    let item = NewJob {
        url,
        title: None,
        thumbnail: None,
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
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    let mut dialog = app
        .dialog()
        .file()
        .set_title("Carpeta de descargas")
        .set_directory(downloads::download_root(&app));
    if let Some(w) = app.get_webview_window("main") {
        dialog = dialog.set_parent(&w);
    }
    dialog.pick_folder(move |picked| {
        let _ = tx.send(picked);
    });
    let path = rx.await.ok()??.into_path().ok()?;
    settings::update(&app, |s| s.folder = Some(path.clone()));
    Some(path.display().to_string())
}

/// Phones keep downloads in Download/Oval; there is no folder picker.
#[cfg(mobile)]
#[tauri::command]
async fn choose_folder() -> Option<String> {
    None
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default().plugin(tauri_plugin_clipboard_manager::init());
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_dialog::init());
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
            #[cfg(windows)]
            {
                let handle = app.handle().clone();
                platform::keep_awake_while(move || handle.state::<Queue>().busy());
            }
            // Warm the engine up front so the first download starts instantly.
            tauri::async_runtime::spawn(engine::warm_up(app.handle().clone()));
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

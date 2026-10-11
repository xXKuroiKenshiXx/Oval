#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod channel;
mod downloads;
mod platform;
mod settings;
mod tools;

use downloads::{Job, Mode, NewJob, Queue};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

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
fn cancel_listing() {
    channel::cancel()
}

#[tauri::command]
fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

#[tauri::command]
fn engine_status() -> tools::EngineStatus {
    tools::status()
}

#[tauri::command]
fn download_folder(app: AppHandle) -> String {
    downloads::download_root(&app).display().to_string()
}

/// Lets the user pick where downloads go; `None` if the dialog was dismissed.
#[tauri::command]
async fn choose_folder(app: AppHandle) -> Option<String> {
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

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Queue::default())
        .manage(settings::Store(std::sync::Mutex::new(settings::load())))
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                platform::click_through_outside_oval(window);
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Warm the engine up front so the first download starts instantly.
                if tools::ensure(&handle).await.is_ok() {
                    tools::update_in_background(handle).await;
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            jobs,
            add_download,
            add_many,
            add_live,
            stop_live,
            pause,
            resume,
            cancel,
            clear_finished,
            reveal,
            get_concurrency,
            set_concurrency,
            list_channel,
            cancel_listing,
            clipboard_text,
            engine_status,
            download_folder,
            choose_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Oval");
}

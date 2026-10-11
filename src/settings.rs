use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub concurrency: u32,
    /// Where downloads go; `None` means `Downloads\Oval`.
    pub folder: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            concurrency: 3,
            folder: None,
        }
    }
}

pub struct Store(pub Mutex<Settings>);

fn file(app: &AppHandle) -> Option<PathBuf> {
    #[cfg(desktop)]
    let _ = app;
    #[cfg(desktop)]
    return Some(dirs::config_dir()?.join("Oval").join("settings.json"));
    #[cfg(mobile)]
    return Some(app.path().app_config_dir().ok()?.join("settings.json"));
}

pub fn load(app: &AppHandle) -> Settings {
    file(app)
        .and_then(|f| std::fs::read_to_string(f).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn get(app: &AppHandle) -> Settings {
    app.state::<Store>().0.lock().unwrap().clone()
}

pub fn update(app: &AppHandle, f: impl FnOnce(&mut Settings)) -> Settings {
    let store = app.state::<Store>();
    let mut s = store.0.lock().unwrap();
    f(&mut s);
    if let Some(path) = file(app) {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, serde_json::to_string_pretty(&*s).unwrap_or_default());
    }
    s.clone()
}

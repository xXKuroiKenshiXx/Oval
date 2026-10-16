//! Phones can't start a yt-dlp process, so this plugin runs it inside the
//! app: through youtubedl-android (embedded Python + yt-dlp + FFmpeg) on
//! Android, and through an embedded Python with JavaScriptCore and
//! AVFoundation on iOS. Desktop builds compile it to nothing.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(mobile)]
mod mobile;
#[cfg(mobile)]
pub use mobile::{Engine, Event, Setup};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_oval_engine);

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("oval-engine")
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                use tauri::Manager;
                #[cfg(target_os = "android")]
                let handle = _api.register_android_plugin(
                    "io.github.xxkuroikenshixx.oval.engine",
                    "EnginePlugin",
                )?;
                #[cfg(target_os = "ios")]
                let handle = _api.register_ios_plugin(init_plugin_oval_engine)?;
                _app.manage(Engine(handle));
            }
            Ok(())
        })
        .build()
}

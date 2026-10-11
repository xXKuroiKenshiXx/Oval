//! On Android there is no yt-dlp executable to spawn, so this plugin runs it
//! through youtubedl-android (an embedded Python + yt-dlp + FFmpeg). Desktop
//! builds compile it to nothing.

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "android")]
pub use android::{Engine, Event};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("oval-engine")
        .setup(|_app, _api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Manager;
                let handle = _api.register_android_plugin(
                    "io.github.xxkuroikenshixx.oval.engine",
                    "EnginePlugin",
                )?;
                _app.manage(Engine(handle));
            }
            Ok(())
        })
        .build()
}

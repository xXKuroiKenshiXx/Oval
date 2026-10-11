use serde::{Deserialize, Serialize};
use tauri::{
    ipc::{Channel, InvokeResponseBody},
    plugin::PluginHandle,
    Runtime,
};

/// Handle to the Kotlin side, stored as app state.
pub struct Engine<R: Runtime>(pub(crate) PluginHandle<R>);

/// One message from a running yt-dlp: a stdout line, a stderr line, or the
/// exit code (always last).
#[derive(Debug, Deserialize)]
pub struct Event {
    pub out: Option<String>,
    pub err: Option<String>,
    pub exit: Option<i32>,
}

#[derive(Serialize)]
struct RunPayload {
    id: u64,
    args: Vec<String>,
    channel: Channel,
}

#[derive(Serialize)]
struct IdPayload {
    id: u64,
}

impl<R: Runtime> Engine<R> {
    /// Unpacks Python and yt-dlp on first launch; returns the yt-dlp version.
    pub fn prepare(&self) -> Result<String, String> {
        #[derive(Deserialize)]
        struct Init {
            version: String,
        }
        self.0
            .run_mobile_plugin::<Init>("init", ())
            .map(|i| i.version)
            .map_err(|e| e.to_string())
    }

    /// Fetches the latest stable yt-dlp.
    pub fn update(&self) -> Result<(), String> {
        self.0
            .run_mobile_plugin::<serde_json::Value>("update", ())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Starts yt-dlp with `args`; `on_event` receives its output until exit.
    pub fn run<F>(&self, id: u64, args: Vec<String>, on_event: F) -> Result<(), String>
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        let channel: Channel = Channel::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                if let Ok(event) = serde_json::from_str::<Event>(&json) {
                    on_event(event);
                }
            }
            Ok(())
        });
        self.0
            .run_mobile_plugin::<serde_json::Value>("run", RunPayload { id, args, channel })
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn kill(&self, id: u64) {
        let _ = self
            .0
            .run_mobile_plugin::<serde_json::Value>("kill", IdPayload { id });
    }
}

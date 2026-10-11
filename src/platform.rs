//! Platform helpers: click-through outside the oval, child processes that die
//! with the app, and process tree termination.

use std::path::Path;
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Configures a helper process: no console window on Windows; on Linux its own
/// process group (so `kill_tree` reaches FFmpeg too) that dies with the app,
/// without the AppImage's library and Python paths.
pub fn prepare(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    #[cfg(unix)]
    for (key, value) in appimage_env_fixes(std::env::vars()) {
        match value {
            Some(v) => cmd.env(key, v),
            None => cmd.env_remove(key),
        };
    }
    #[cfg(unix)]
    unsafe {
        cmd.process_group(0);
        cmd.pre_exec(|| {
            #[cfg(target_os = "linux")]
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
            Ok(())
        });
    }
}

/// Inside an AppImage, the launcher points LD_LIBRARY_PATH, PYTHONHOME and the
/// like at the bundled libraries. yt-dlp, FFmpeg and Deno bring their own and
/// must not pick those up: returns the variables to drop (`None`) or rewrite
/// (PATH-like lists, minus the AppImage's entries).
#[cfg(unix)]
fn appimage_env_fixes(
    vars: impl Iterator<Item = (String, String)>,
) -> Vec<(String, Option<String>)> {
    let vars: Vec<(String, String)> = vars.collect();
    let Some(appdir) = vars
        .iter()
        .find(|(k, _)| k == "APPDIR")
        .map(|(_, v)| v.trim_end_matches('/').to_string())
        .filter(|d| !d.is_empty())
    else {
        return Vec::new();
    };
    let mut fixes = Vec::new();
    for (key, value) in vars {
        if !value.contains(&appdir) {
            continue;
        }
        let kept: Vec<&str> = value
            .split(':')
            .filter(|part| !part.is_empty() && !part.starts_with(&appdir))
            .collect();
        let keep_rest = matches!(key.as_str(), "PATH" | "XDG_DATA_DIRS");
        fixes.push((key, (keep_rest && !kept.is_empty()).then(|| kept.join(":"))));
    }
    fixes
}

/// Inset of the CSS oval inside the transparent window, in logical pixels.
const OVAL_INSET: f64 = 14.0;

/// Lets clicks outside the oval fall through to whatever is behind the
/// transparent window. A light poll of the cursor toggles Tauri's
/// ignore-cursor-events flag; unlike a window region this keeps the
/// anti-aliased edge and causes no DWM artifacts.
pub fn click_through_outside_oval(window: tauri::WebviewWindow) {
    std::thread::spawn(move || {
        let mut ignoring = false;
        loop {
            std::thread::sleep(std::time::Duration::from_millis(30));
            let (Ok(cursor), Ok(pos), Ok(size), Ok(scale)) = (
                window.cursor_position(),
                window.outer_position(),
                window.inner_size(),
                window.scale_factor(),
            ) else {
                if window.is_visible().is_err() {
                    break; // window closed
                }
                continue;
            };
            let (w, h) = (size.width as f64, size.height as f64);
            let a = w / 2.0 - OVAL_INSET * scale;
            let b = h / 2.0 - OVAL_INSET * scale;
            let dx = (cursor.x - pos.x as f64 - w / 2.0) / a;
            let dy = (cursor.y - pos.y as f64 - h / 2.0) / b;
            let outside = dx * dx + dy * dy > 1.0;
            if outside != ignoring && window.set_ignore_cursor_events(outside).is_ok() {
                ignoring = outside;
            }
        }
    });
}

#[cfg(windows)]
mod job {
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::*;

    struct Job(HANDLE);
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    static JOB: OnceLock<Option<Job>> = OnceLock::new();

    fn handle() -> Option<HANDLE> {
        JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            Some(Job(job))
        })
        .as_ref()
        .map(|j| j.0)
    }

    pub fn adopt(process: HANDLE) {
        if let Some(job) = handle() {
            unsafe {
                AssignProcessToJobObject(job, process);
            }
        }
    }
}

/// Ties a child process (and everything it spawns) to the app's lifetime.
#[cfg(windows)]
pub fn adopt_child(child: &tokio::process::Child) {
    if let Some(h) = child.raw_handle() {
        job::adopt(h as _);
    }
}

#[cfg(not(windows))]
pub fn adopt_child(_child: &tokio::process::Child) {}

/// Keeps Windows from going to sleep while `busy` says downloads are running:
/// an idle PC that sleeps mid-download cuts it.
#[cfg(windows)]
pub fn keep_awake_while(busy: impl Fn() -> bool + Send + 'static) {
    use windows_sys::Win32::System::Power::{SetThreadExecutionState, ES_SYSTEM_REQUIRED};
    std::thread::spawn(move || loop {
        if busy() {
            // Resets the idle timer, the way media players do.
            unsafe {
                SetThreadExecutionState(ES_SYSTEM_REQUIRED);
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(30));
    });
}

/// Kills a process together with any helpers it started (ffmpeg, etc.).
pub fn kill_tree(pid: u32) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(not(windows))]
    {
        // Negative pid = the whole process group set up in `prepare`. A syscall
        // rather than the `kill` program, which minimal runtimes (Flatpak) may lack.
        if let Ok(pgid) = libc::pid_t::try_from(pid) {
            // SAFETY: plain kill(2) on a process group Oval created.
            unsafe {
                libc::kill(-pgid, libc::SIGKILL);
            }
        }
    }
}

/// Opens Explorer with `path` selected, or the folder itself.
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if path.is_file() {
            let _ = Command::new("explorer")
                .raw_arg(format!("/select,\"{}\"", path.display()))
                .spawn();
        } else {
            let _ = Command::new("explorer").arg(path).spawn();
        }
    }
    #[cfg(not(windows))]
    {
        let target = if path.is_file() {
            path.parent().unwrap_or(path)
        } else {
            path
        };
        let _ = Command::new("xdg-open").arg(target).spawn();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::appimage_env_fixes;

    #[test]
    fn appimage_paths_stay_out_of_helpers() {
        let vars = [
            ("APPDIR", "/tmp/.mount_Oval"),
            ("PATH", "/tmp/.mount_Oval/usr/bin/:/usr/bin:/bin"),
            ("LD_LIBRARY_PATH", "/tmp/.mount_Oval/usr/lib/:"),
            ("PYTHONHOME", "/tmp/.mount_Oval/usr/"),
            ("HOME", "/home/me"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let fixes = appimage_env_fixes(vars.into_iter());
        let get = |k: &str| {
            fixes
                .iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("PATH"), Some(Some("/usr/bin:/bin".into())));
        assert_eq!(get("LD_LIBRARY_PATH"), Some(None));
        assert_eq!(get("PYTHONHOME"), Some(None));
        assert_eq!(get("APPDIR"), Some(None));
        assert_eq!(get("HOME"), None);
        assert!(appimage_env_fixes(std::iter::empty()).is_empty());
    }
}

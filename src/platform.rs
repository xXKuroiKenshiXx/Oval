//! Platform helpers: click-through outside the oval, child processes that die
//! with the app, and process tree termination.

use std::path::Path;
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Configures a helper process: no console window on Windows; on Linux its own
/// process group (so `kill_tree` reaches FFmpeg too) that dies with the app.
pub fn prepare(cmd: &mut tokio::process::Command) {
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
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

//! Live Headset Window placement and per-user feeder placement preferences.
//! Only windows titled Headset Window owned by vrcompositor.exe are targeted.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// One compositor output window, in physical desktop pixels.
#[derive(Clone, Debug, Serialize)]
pub struct WindowInfo {
    /// Native handle, useful for detecting window recreation.
    pub handle: usize,
    /// Owning process id.
    pub process_id: u32,
    /// Left, top, width, height.
    pub bounds: [i32; 4],
}
/// One monitor in the virtual desktop coordinate system.
#[derive(Clone, Debug, Serialize)]
pub struct MonitorInfo {
    /// Windows display device name.
    pub name: String,
    /// Left, top, width, height in physical pixels.
    pub bounds: [i32; 4],
    /// Whether this is the primary monitor.
    pub primary: bool,
}
/// Query result shared by GUI and CUI.
#[derive(Clone, Debug, Serialize)]
pub struct Status {
    /// Schema version.
    pub schema_version: u32,
    /// Matching windows; usually zero or one.
    pub windows: Vec<WindowInfo>,
    /// Available monitors, ordered by their Windows device names.
    pub monitors: Vec<MonitorInfo>,
    /// Saved position applied by the feeder when a window appears.
    pub saved_position: Option<[i32; 2]>,
    /// Last automatic placement error, if any.
    pub last_error: Option<String>,
}
/// Window operation available over the control pipe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    /// Query position and monitors.
    Status,
    /// Move the current window immediately.
    Move([i32; 2]),
    /// Move to the origin of a monitor by index.
    Monitor(usize),
    /// Save a position and apply it when a window is available.
    Save([i32; 2]),
    /// Forget the feeder's saved placement.
    Forget,
}
impl WindowCommand {
    /// Parse arguments following `window`.
    pub fn parse(args: &[&str]) -> Result<Self, String> {
        match args {
            ["status"] => Ok(Self::Status),
            ["forget"] => Ok(Self::Forget),
            [verb @ ("move" | "save"), x, y] => {
                let pos = [
                    x.parse().map_err(|_| "x must be an integer")?,
                    y.parse().map_err(|_| "y must be an integer")?,
                ];
                Ok(if *verb == "move" {
                    Self::Move(pos)
                } else {
                    Self::Save(pos)
                })
            }
            ["monitor", n] => Ok(Self::Monitor(
                n.parse().map_err(|_| "monitor index must be an integer")?,
            )),
            _ => Err(
                "window status | move <x> <y> | monitor <index> | save <x> <y> | forget".to_owned(),
            ),
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Preference {
    position: [i32; 2],
}
fn preference_path() -> Result<PathBuf, String> {
    let root = std::env::var_os("VRDUMMY_STATE_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("vrdummy")))
        .ok_or("no per-user application state directory")?;
    Ok(root.join("headset-window.json"))
}

/// Tracks window recreation so a saved position is applied once per window.
pub struct Manager {
    saved: Option<[i32; 2]>,
    applied: Option<usize>,
    last_poll: Option<Instant>,
    error: Option<String>,
}
impl Default for Manager {
    fn default() -> Self {
        // Unit tests must not apply a user's saved placement to real windows.
        let loaded = if cfg!(test) {
            Ok(None)
        } else {
            preference_path().and_then(|path| match std::fs::read(path) {
                Ok(bytes) => serde_json::from_slice::<Preference>(&bytes)
                    .map(|p| Some(p.position))
                    .map_err(|e| e.to_string()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e.to_string()),
            })
        };
        let (saved, error) = match loaded {
            Ok(p) => (p, None),
            Err(e) => (None, Some(e)),
        };
        Self {
            saved,
            applied: None,
            last_poll: None,
            error,
        }
    }
}
impl Manager {
    /// Read native window and monitor positions.
    pub fn status(&self) -> Result<Status, String> {
        let (windows, monitors) = native::enumerate()?;
        Ok(Status {
            schema_version: 1,
            windows,
            monitors,
            saved_position: self.saved,
            last_error: self.error.clone(),
        })
    }
    /// Apply stored placement to a newly created window, at most once a second.
    pub fn poll(&mut self) {
        if self.saved.is_none()
            || self
                .last_poll
                .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            return;
        }
        self.last_poll = Some(Instant::now());
        match self.status() {
            Ok(s) if s.windows.is_empty() => self.applied = None,
            Ok(s) if s.windows.len() == 1 && self.applied != Some(s.windows[0].handle) => {
                if let Some(pos) = self.saved {
                    match move_current(&s, pos) {
                        Ok(()) => {
                            self.applied = Some(s.windows[0].handle);
                            self.error = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            Err(e) => self.error = Some(e),
            _ => {}
        }
    }
    /// Execute a live or persisted placement operation.
    pub fn execute(&mut self, command: &WindowCommand) -> Result<Status, String> {
        let status = self.status()?;
        match command {
            WindowCommand::Status => {}
            WindowCommand::Move(pos) => {
                move_current(&status, *pos)?;
                self.applied = status.windows.first().map(|w| w.handle);
            }
            WindowCommand::Monitor(index) => {
                let m = status
                    .monitors
                    .get(*index)
                    .ok_or("monitor index out of range")?;
                move_current(&status, [m.bounds[0], m.bounds[1]])?;
                self.applied = status.windows.first().map(|w| w.handle);
            }
            WindowCommand::Save(pos) => {
                if !visible(*pos, [32, 32], &status.monitors) {
                    return Err("saved position must intersect an available monitor".to_owned());
                }
                let path = preference_path()?;
                let parent = path.parent().ok_or("invalid state path")?;
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                let temporary = parent.join(format!("headset-window-{}.tmp", std::process::id()));
                let bytes = serde_json::to_vec(&Preference { position: *pos })
                    .map_err(|e| e.to_string())?;
                std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
                if let Err(e) = std::fs::rename(&temporary, &path) {
                    let _ = std::fs::remove_file(&temporary);
                    return Err(e.to_string());
                }
                self.saved = Some(*pos);
                self.applied = None;
                self.last_poll = None;
                self.poll();
            }
            WindowCommand::Forget => {
                match std::fs::remove_file(preference_path()?) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.to_string()),
                }
                self.saved = None;
                self.applied = None;
                self.error = None;
            }
        }
        self.status()
    }
}
fn visible(pos: [i32; 2], size: [i32; 2], monitors: &[MonitorInfo]) -> bool {
    monitors.iter().any(|m| {
        let [x, y, w, h] = m.bounds.map(i64::from);
        let [px, py] = pos.map(i64::from);
        px + i64::from(size[0]) > x && py + i64::from(size[1]) > y && px < x + w && py < y + h
    })
}
fn move_current(status: &Status, position: [i32; 2]) -> Result<(), String> {
    let [window] = status.windows.as_slice() else {
        return Err(
            "expected one SteamVR Headset Window; start SteamVR with the dummy HMD".to_owned(),
        );
    };
    if !visible(
        position,
        [window.bounds[2], window.bounds[3]],
        &status.monitors,
    ) {
        return Err("window would be outside every monitor".to_owned());
    }
    native::move_window(window.handle, position)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn negative_monitor_origins_and_large_coordinates_do_not_overflow() {
        let monitors = [MonitorInfo {
            name: "left".to_owned(),
            bounds: [-1920, -120, 1920, 1080],
            primary: false,
        }];
        assert!(visible([-1900, 0], [500, 500], &monitors));
        assert!(visible([-2000, 0], [500, 500], &monitors));
        assert!(!visible([0, 0], [500, 500], &monitors));
        assert!(!visible([i32::MAX, i32::MAX], [500, 500], &monitors));
        assert!(!visible([i32::MIN, i32::MIN], [500, 500], &monitors));
    }
    #[test]
    fn window_commands_reject_invalid_arguments() {
        assert_eq!(
            WindowCommand::parse(&["move", "-1920", "80"]).unwrap(),
            WindowCommand::Move([-1920, 80])
        );
        for args in [
            vec!["move", "NaN", "0"],
            vec!["save", "2147483648", "0"],
            vec!["monitor", "-1"],
            vec!["status", "extra"],
        ] {
            assert!(WindowCommand::parse(&args).is_err());
        }
    }
}

#[cfg(not(windows))]
mod native {
    use super::{MonitorInfo, WindowInfo};
    pub fn enumerate() -> Result<(Vec<WindowInfo>, Vec<MonitorInfo>), String> {
        Err("Headset Window control requires Windows".to_owned())
    }
    pub fn move_window(_: usize, _: [i32; 2]) -> Result<(), String> {
        Err("Headset Window control requires Windows".to_owned())
    }
}

#[cfg(windows)]
mod native {
    use super::{MonitorInfo, WindowInfo};
    use std::ffi::c_void;
    type Handle = *mut c_void;
    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }
    #[repr(C)]
    struct Monitor {
        size: u32,
        rect: Rect,
        work: Rect,
        flags: u32,
        name: [u16; 32],
    }
    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(
            callback: unsafe extern "system" fn(Handle, isize) -> i32,
            data: isize,
        ) -> i32;
        fn GetWindowTextW(hwnd: Handle, text: *mut u16, count: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: Handle, pid: *mut u32) -> u32;
        fn GetWindowRect(hwnd: Handle, rect: *mut Rect) -> i32;
        fn SetWindowPos(
            hwnd: Handle,
            after: Handle,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: u32,
        ) -> i32;
        fn EnumDisplayMonitors(
            hdc: Handle,
            clip: *const Rect,
            callback: unsafe extern "system" fn(Handle, Handle, *mut Rect, isize) -> i32,
            data: isize,
        ) -> i32;
        fn GetMonitorInfoW(monitor: Handle, info: *mut Monitor) -> i32;
        fn SetThreadDpiAwarenessContext(context: Handle) -> Handle;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> Handle;
        fn QueryFullProcessImageNameW(
            process: Handle,
            flags: u32,
            name: *mut u16,
            size: *mut u32,
        ) -> i32;
        fn CloseHandle(handle: Handle) -> i32;
    }
    struct Dpi(Handle);
    impl Dpi {
        fn physical() -> Self {
            Self(unsafe { SetThreadDpiAwarenessContext(-4isize as Handle) })
        }
    }
    impl Drop for Dpi {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    SetThreadDpiAwarenessContext(self.0);
                }
            }
        }
    }
    fn compositor(hwnd: Handle) -> Option<u32> {
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &raw mut pid);
        }
        // GetWindowTextW sends WM_GETTEXT for our own windows. Skip them to
        // avoid re-entering winit while egui holds its UI state lock.
        if pid == std::process::id() {
            return None;
        }
        let mut title = [0u16; 256];
        let n = unsafe { GetWindowTextW(hwnd, title.as_mut_ptr(), 256) };
        if n <= 0 || String::from_utf16_lossy(&title[..n as usize]) != "Headset Window" {
            return None;
        }
        let process = unsafe { OpenProcess(0x1000, 0, pid) };
        if process.is_null() {
            return None;
        }
        let mut path = vec![0u16; 32768];
        let mut len = path.len() as u32;
        let ok = unsafe { QueryFullProcessImageNameW(process, 0, path.as_mut_ptr(), &raw mut len) };
        unsafe {
            CloseHandle(process);
        }
        if ok == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&path[..len as usize]);
        path.rsplit(['\\', '/'])
            .next()
            .is_some_and(|s| s.eq_ignore_ascii_case("vrcompositor.exe"))
            .then_some(pid)
    }
    unsafe extern "system" fn window_callback(hwnd: Handle, data: isize) -> i32 {
        if let Some(pid) = compositor(hwnd) {
            let mut rect = Rect::default();
            if unsafe { GetWindowRect(hwnd, &raw mut rect) } != 0 {
                let windows = unsafe { &mut *(data as *mut Vec<WindowInfo>) };
                windows.push(WindowInfo {
                    handle: hwnd as usize,
                    process_id: pid,
                    bounds: [
                        rect.left,
                        rect.top,
                        rect.right - rect.left,
                        rect.bottom - rect.top,
                    ],
                });
            }
        }
        1
    }
    unsafe extern "system" fn monitor_callback(
        handle: Handle,
        _: Handle,
        _: *mut Rect,
        data: isize,
    ) -> i32 {
        let mut info = Monitor {
            size: std::mem::size_of::<Monitor>() as u32,
            rect: Rect::default(),
            work: Rect::default(),
            flags: 0,
            name: [0; 32],
        };
        if unsafe { GetMonitorInfoW(handle, &raw mut info) } != 0 {
            let monitors = unsafe { &mut *(data as *mut Vec<MonitorInfo>) };
            let n = info.name.iter().position(|c| *c == 0).unwrap_or(32);
            let r = info.rect;
            monitors.push(MonitorInfo {
                name: String::from_utf16_lossy(&info.name[..n]),
                bounds: [r.left, r.top, r.right - r.left, r.bottom - r.top],
                primary: info.flags & 1 != 0,
            });
        }
        1
    }
    pub fn enumerate() -> Result<(Vec<WindowInfo>, Vec<MonitorInfo>), String> {
        let _dpi = Dpi::physical();
        let mut windows = Vec::new();
        let mut monitors = Vec::new();
        if unsafe { EnumWindows(window_callback, (&raw mut windows) as isize) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if unsafe {
            EnumDisplayMonitors(
                std::ptr::null_mut(),
                std::ptr::null(),
                monitor_callback,
                (&raw mut monitors) as isize,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        monitors.sort_by(|a: &MonitorInfo, b| a.name.cmp(&b.name));
        Ok((windows, monitors))
    }
    pub fn move_window(handle: usize, position: [i32; 2]) -> Result<(), String> {
        let _dpi = Dpi::physical();
        let hwnd = handle as Handle;
        if compositor(hwnd).is_none() {
            return Err("Headset Window was closed or replaced".to_owned());
        }
        // ASYNCWINDOWPOS | NOSIZE | NOZORDER | NOACTIVATE: do not steal input focus.
        if unsafe {
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                position[0],
                position[1],
                0,
                0,
                0x4015,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        for _ in 0..20 {
            let mut r = Rect::default();
            if unsafe { GetWindowRect(hwnd, &raw mut r) } != 0 && [r.left, r.top] == position {
                return Ok(());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Err(
            "SteamVR did not retain the requested position; refresh to inspect the actual position"
                .to_owned(),
        )
    }
}

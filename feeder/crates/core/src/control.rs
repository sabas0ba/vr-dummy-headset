//! Control API: a local named pipe accepting one command per line and
//! answering one line per command (`ok [text]`, `err <text>` or `quit`).
//!
//! The server runs on its own thread and locks the shared engine for each
//! command. Clients connect sequentially; `vrdummy-cli set` is one client.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use crate::engine::{Engine, Reply};
use crate::win32;

/// Shared engine handle used by front ends and the control server.
pub type SharedEngine = Arc<Mutex<Engine>>;

/// Running control server.
pub struct ControlServer {
    pipe_name: String,
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    /// Starts serving `pipe_name` on a background thread.
    pub fn start(pipe_name: &str, engine: SharedEngine, quit_flag: Arc<AtomicBool>) -> io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let name = pipe_name.to_owned();
        let flag = Arc::clone(&running);
        let thread = std::thread::Builder::new()
            .name("vrdummy-control".to_owned())
            .spawn(move || serve(&name, &engine, &flag, &quit_flag))?;
        log::info!("control pipe listening on {}", win32::pipe_path(pipe_name));
        Ok(Self {
            pipe_name: pipe_name.to_owned(),
            running,
            thread: Some(thread),
        })
    }

    /// Stops the server. Unblocks the accept call by connecting to it.
    pub fn stop(&mut self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        let _ = File::options()
            .read(true)
            .write(true)
            .open(win32::pipe_path(&self.pipe_name));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for ControlServer {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Handles one line and returns the reply line.
pub fn handle_line(engine: &SharedEngine, line: &str) -> (String, bool) {
    let Ok(mut engine) = engine.lock() else {
        return ("err engine lock poisoned".to_owned(), false);
    };
    match engine.execute_line(line) {
        Ok(Reply::Quit) => ("quit".to_owned(), true),
        Ok(reply) => (reply.to_line(), false),
        Err(e) => (format!("err {e}"), false),
    }
}

#[cfg(windows)]
fn create_server_pipe(pipe_name: &str) -> io::Result<File> {
    use std::os::windows::io::FromRawHandle;
    let path = win32::wide(&win32::pipe_path(pipe_name));
    // SAFETY: `path` is NUL-terminated and outlives the call; all other
    // arguments are plain values. The returned handle is owned by the File.
    let handle = unsafe {
        win32::CreateNamedPipeW(
            path.as_ptr(),
            win32::PIPE_ACCESS_DUPLEX,
            win32::PIPE_TYPE_BYTE | win32::PIPE_READMODE_BYTE | win32::PIPE_WAIT | win32::PIPE_REJECT_REMOTE_CLIENTS,
            1,
            4096,
            4096,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == win32::INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `handle` is a valid, owned pipe handle returned above.
    let ok = unsafe { win32::ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if ok == 0 {
        // SAFETY: GetLastError has no preconditions.
        let err = unsafe { win32::GetLastError() };
        if err != win32::ERROR_PIPE_CONNECTED {
            // SAFETY: closing the handle we own exactly once.
            unsafe { win32::CloseHandle(handle) };
            return Err(io::Error::from_raw_os_error(err as i32));
        }
    }
    // SAFETY: ownership of the handle moves into the File, which closes it.
    Ok(unsafe { File::from_raw_handle(handle.cast()) })
}

#[cfg(not(windows))]
fn create_server_pipe(_pipe_name: &str) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "control pipe is only implemented on Windows",
    ))
}

fn serve(pipe_name: &str, engine: &SharedEngine, running: &AtomicBool, quit_flag: &AtomicBool) {
    while running.load(Ordering::SeqCst) {
        let file = match create_server_pipe(pipe_name) {
            Ok(f) => f,
            Err(e) => {
                log::error!("control pipe failed: {e}");
                std::thread::sleep(std::time::Duration::from_secs(1));
                continue;
            }
        };
        if !running.load(Ordering::SeqCst) {
            break;
        }
        let mut writer = match file.try_clone() {
            Ok(w) => w,
            Err(e) => {
                log::error!("control pipe clone failed: {e}");
                continue;
            }
        };
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let Ok(line) = line else { break };
            let (reply, quit) = handle_line(engine, &line);
            if writeln!(writer, "{reply}").is_err() || writer.flush().is_err() {
                break;
            }
            if quit {
                quit_flag.store(true, Ordering::SeqCst);
                break;
            }
        }
        let _ = writer.flush();
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: the handle is valid for the lifetime of `writer`.
            unsafe {
                win32::FlushFileBuffers(writer.as_raw_handle().cast());
                win32::DisconnectNamedPipe(writer.as_raw_handle().cast());
            }
        }
    }
}

/// Opens the control pipe as a client, waiting while the server is between
/// connections (busy) or not yet listening (not found), up to `timeout`.
fn open_client(pipe_name: &str, timeout: std::time::Duration) -> io::Result<File> {
    let path = win32::pipe_path(pipe_name);
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match File::options().read(true).write(true).open(&path) {
            Ok(f) => return Ok(f),
            Err(e) => {
                let code = e.raw_os_error().unwrap_or(0) as u32;
                if std::time::Instant::now() >= deadline
                    || (code != win32::ERROR_PIPE_BUSY && code != win32::ERROR_FILE_NOT_FOUND)
                {
                    return Err(e);
                }
                #[cfg(windows)]
                if code == win32::ERROR_PIPE_BUSY {
                    let wide = win32::wide(&path);
                    // SAFETY: `wide` is NUL-terminated and outlives the call.
                    unsafe { win32::WaitNamedPipeW(wide.as_ptr(), 200) };
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        }
    }
}

/// Sends one line to a running control pipe and returns the reply line.
/// Waits up to two seconds for the server to accept the connection.
pub fn send_line(pipe_name: &str, line: &str) -> io::Result<String> {
    let file = open_client(pipe_name, std::time::Duration::from_secs(2))?;
    let mut writer = file.try_clone()?;
    writeln!(writer, "{}", line.trim())?;
    writer.flush()?;
    let mut reply = String::new();
    BufReader::new(file).read_line(&mut reply)?;
    Ok(reply.trim_end().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn handle_line_formats_replies() {
        let engine: SharedEngine = Arc::new(Mutex::new(Engine::new(Config::default())));
        assert_eq!(handle_line(&engine, "pos hmd 0 1 0"), ("ok".to_owned(), false));
        assert!(handle_line(&engine, "status").0.starts_with("ok connected=false"));
        assert!(handle_line(&engine, "fly").0.starts_with("err unknown command"));
        assert_eq!(handle_line(&engine, "quit"), ("quit".to_owned(), true));
    }

    #[cfg(windows)]
    #[test]
    fn round_trip_over_pipe() {
        let engine: SharedEngine = Arc::new(Mutex::new(Engine::new(Config::default())));
        let quit = Arc::new(AtomicBool::new(false));
        let name = format!("vrdummy-test-{}", std::process::id());
        let mut server = ControlServer::start(&name, Arc::clone(&engine), Arc::clone(&quit)).expect("start");
        assert_eq!(send_line(&name, "pos hmd 0 2 0").expect("first reply"), "ok");
        assert!(send_line(&name, "status").expect("second reply").starts_with("ok connected=false"));
        assert_eq!(engine.lock().expect("lock").world().pose(0).map(|p| p.position[1]), Some(2.0));
        assert_eq!(send_line(&name, "quit").expect("reply"), "quit");
        assert!(quit.load(Ordering::SeqCst));
        server.stop();
    }
}

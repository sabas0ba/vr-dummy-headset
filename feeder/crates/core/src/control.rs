//! Control API: a local named pipe accepting one command per line and
//! answering one line per command (`ok [text]`, `err <text>` or `quit`).
//!
//! The server runs on its own thread and locks the shared engine for each
//! command. Clients connect sequentially; `vrdummy-cli set` is one client.

#[cfg(all(test, windows))]
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
    running: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ControlServer {
    /// Starts serving `pipe_name` on a background thread.
    pub fn start(
        pipe_name: &str,
        engine: SharedEngine,
        quit_flag: Arc<AtomicBool>,
    ) -> io::Result<Self> {
        let running = Arc::new(AtomicBool::new(true));
        let name = pipe_name.to_owned();
        let flag = Arc::clone(&running);
        let (ready_tx, ready_rx) = std::sync::mpsc::sync_channel(0);
        let thread = std::thread::Builder::new()
            .name("vrdummy-control".to_owned())
            .spawn(move || {
                let _ = ready_tx.send(());
                serve(&name, &engine, &flag, &quit_flag);
            })?;
        // Only cancel I/O after Rust has finished initializing the worker.
        // An immediate stop must not interrupt runtime thread startup I/O.
        ready_rx
            .recv()
            .map_err(|_| io::Error::other("control thread failed to start"))?;
        log::info!("control pipe listening on {}", win32::pipe_path(pipe_name));
        Ok(Self {
            running,
            thread: Some(thread),
        })
    }

    /// Stops the server and cancels pending pipe I/O on its dedicated thread.
    pub fn stop(&mut self) {
        if !self.running.swap(false, Ordering::SeqCst) {
            return;
        }
        if let Some(t) = self.thread.take() {
            // Cancellation only affects an I/O already in progress. Repeat until
            // exit to cover stop racing with entry into accept, read or write.
            while !t.is_finished() {
                #[cfg(windows)]
                {
                    use std::os::windows::io::AsRawHandle;
                    // SAFETY: the JoinHandle owns a live thread handle until join.
                    unsafe { win32::CancelSynchronousIo(t.as_raw_handle().cast()) };
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
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

fn serve(pipe_name: &str, engine: &SharedEngine, running: &AtomicBool, quit_flag: &AtomicBool) {
    while running.load(Ordering::SeqCst) {
        let file = match win32::accept_named_pipe(pipe_name) {
            Ok(f) => f,
            Err(e) => {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
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
            if !running.load(Ordering::SeqCst) {
                break;
            }
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
        if running.load(Ordering::SeqCst) {
            let _ = writer.flush();
            win32::disconnect_named_pipe(&writer);
        }
    }
}

/// Opens the control pipe as a client, waiting while the server is between
/// connections (busy) or not yet listening (not found), up to `timeout`.
#[cfg(all(test, windows))]
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
/// The two-second deadline covers connection, writing and reading the reply.
pub fn send_line(pipe_name: &str, line: &str) -> io::Result<String> {
    send_line_with_timeout(pipe_name, line, std::time::Duration::from_secs(2))
}

/// Sends one command with an overall deadline. A timeout does not prove that
/// the server did not execute the command; callers must not retry blindly.
pub fn send_line_with_timeout(
    pipe_name: &str,
    line: &str,
    timeout: std::time::Duration,
) -> io::Result<String> {
    crate::control_client::send(pipe_name, line, timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn handle_line_formats_replies() {
        let engine: SharedEngine = Arc::new(Mutex::new(Engine::new(Config::default())));
        assert_eq!(
            handle_line(&engine, "pos hmd 0 1 0"),
            ("ok".to_owned(), false)
        );
        assert!(
            handle_line(&engine, "status")
                .0
                .starts_with("ok connected=false")
        );
        assert!(
            handle_line(&engine, "fly")
                .0
                .starts_with("err unknown command")
        );
        assert_eq!(handle_line(&engine, "quit"), ("quit".to_owned(), true));
    }

    #[cfg(windows)]
    #[test]
    fn stop_cancels_idle_and_partial_clients() {
        use std::sync::mpsc;
        use std::time::Duration;

        for (index, partial) in ["", "pos hmd 0 2"].iter().enumerate() {
            let engine = Arc::new(Mutex::new(Engine::new(Config::default())));
            let name = format!("vrdummy-stop-{}-{index}", std::process::id());
            let mut server = ControlServer::start(&name, engine, Arc::new(AtomicBool::new(false)))
                .expect("start");
            let mut client = open_client(&name, Duration::from_secs(2)).expect("connect");
            // A round trip establishes that the server is handling this client.
            writeln!(client, "status").expect("status");
            let mut reply = String::new();
            BufReader::new(client.try_clone().expect("clone"))
                .read_line(&mut reply)
                .expect("reply");
            assert!(reply.starts_with("ok "));
            client.write_all(partial.as_bytes()).expect("partial line");
            let (tx, rx) = mpsc::channel();
            let stopper = std::thread::spawn(move || {
                server.stop();
                server.stop();
                let _ = tx.send(());
            });
            let stopped = rx.recv_timeout(Duration::from_secs(2));
            // Release the client on failure so the old implementation can exit.
            drop(client);
            assert!(
                stopped.is_ok(),
                "stop blocked with partial line {partial:?}"
            );
            stopper.join().expect("stop thread");
        }
    }

    #[cfg(windows)]
    #[test]
    fn stop_cancels_accept_and_startup_races() {
        use std::sync::mpsc;
        use std::time::Duration;

        for index in 0..16 {
            let engine = Arc::new(Mutex::new(Engine::new(Config::default())));
            let name = format!("vrdummy-stop-accept-{}-{index}", std::process::id());
            let mut server = ControlServer::start(&name, engine, Arc::new(AtomicBool::new(false)))
                .expect("start");
            if index % 2 == 0 {
                std::thread::sleep(Duration::from_millis(20));
            }
            let (tx, rx) = mpsc::channel();
            let stopper = std::thread::spawn(move || {
                server.stop();
                let _ = tx.send(());
            });
            assert!(
                rx.recv_timeout(Duration::from_secs(2)).is_ok(),
                "bounded stop, iteration {index}"
            );
            stopper.join().expect("stop thread");
        }
    }

    #[cfg(windows)]
    #[test]
    fn round_trip_over_pipe() {
        let engine: SharedEngine = Arc::new(Mutex::new(Engine::new(Config::default())));
        let quit = Arc::new(AtomicBool::new(false));
        let name = format!("vrdummy-test-{}", std::process::id());
        let mut server =
            ControlServer::start(&name, Arc::clone(&engine), Arc::clone(&quit)).expect("start");
        assert_eq!(
            send_line(&name, "pos hmd 0 2 0").expect("first reply"),
            "ok"
        );
        assert!(
            send_line(&name, "status")
                .expect("second reply")
                .starts_with("ok connected=false")
        );
        assert_eq!(
            engine
                .lock()
                .expect("lock")
                .world()
                .pose(0)
                .map(|p| p.position[1]),
            Some(2.0)
        );
        assert_eq!(send_line(&name, "quit").expect("reply"), "quit");
        assert!(quit.load(Ordering::SeqCst));
        server.stop();
    }
}

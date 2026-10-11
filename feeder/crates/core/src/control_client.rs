//! Deadline-bound control pipe client. The deadline covers all I/O stages.

use std::io;
use std::time::Duration;

#[cfg(not(windows))]
pub(crate) fn send(_name: &str, _line: &str, _timeout: Duration) -> io::Result<String> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "control pipes require Windows",
    ))
}

#[cfg(windows)]
pub(crate) use windows::send;

#[cfg(windows)]
mod windows {
    use super::{Duration, io};
    use crate::win32::{ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE, pipe_path};
    use std::ffi::c_void;
    use std::fs::{File, OpenOptions};
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use std::time::Instant;

    const MAX_FRAME: usize = 1024 * 1024;
    const ERROR_IO_PENDING: i32 = 997;
    const WAIT_TIMEOUT: u32 = 258;

    #[repr(C)]
    #[derive(Default)]
    struct Overlapped {
        internal: usize,
        internal_high: usize,
        offset: u32,
        offset_high: u32,
        event: HANDLE,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateEventW(
            attributes: *mut c_void,
            manual: i32,
            initial: i32,
            name: *const u16,
        ) -> HANDLE;
        fn ReadFile(
            file: HANDLE,
            buffer: *mut c_void,
            size: u32,
            read: *mut u32,
            ov: *mut Overlapped,
        ) -> i32;
        fn WriteFile(
            file: HANDLE,
            buffer: *const c_void,
            size: u32,
            written: *mut u32,
            ov: *mut Overlapped,
        ) -> i32;
        fn GetOverlappedResult(
            file: HANDLE,
            ov: *mut Overlapped,
            transferred: *mut u32,
            wait: i32,
        ) -> i32;
        fn CancelIoEx(file: HANDLE, ov: *mut Overlapped) -> i32;
        fn WaitForSingleObject(handle: HANDLE, milliseconds: u32) -> u32;
    }

    fn timeout_error() -> io::Error {
        io::Error::new(
            io::ErrorKind::TimedOut,
            "control request deadline exceeded; command outcome may be unknown",
        )
    }

    fn remaining(start: Instant, timeout: Duration) -> io::Result<Duration> {
        timeout
            .checked_sub(start.elapsed())
            .filter(|left| !left.is_zero())
            .ok_or_else(timeout_error)
    }

    fn open(name: &str, start: Instant, timeout: Duration) -> io::Result<File> {
        loop {
            let left = remaining(start, timeout)?;
            match OpenOptions::new()
                .read(true)
                .write(true)
                .custom_flags(0x4000_0000)
                .open(pipe_path(name))
            {
                Ok(file) => return Ok(file),
                Err(error) if matches!(error.raw_os_error(), Some(code) if code == ERROR_PIPE_BUSY as i32 || code == ERROR_FILE_NOT_FOUND as i32) =>
                {
                    std::thread::sleep(left.min(Duration::from_millis(10)));
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn transfer(
        file: &File,
        buffer: &mut [u8],
        writing: bool,
        start: Instant,
        timeout: Duration,
    ) -> io::Result<usize> {
        let left = remaining(start, timeout)?;
        // SAFETY: a new unnamed, manual-reset event; OwnedHandle closes it once.
        let event = unsafe { CreateEventW(std::ptr::null_mut(), 1, 0, std::ptr::null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        let event = unsafe { OwnedHandle::from_raw_handle(event) };
        let mut ov = Overlapped {
            event: event.as_raw_handle(),
            ..Overlapped::default()
        };
        let handle = file.as_raw_handle();
        let mut transferred = 0;
        // SAFETY: file, event, buffer and OVERLAPPED remain live until completion,
        // including cancellation. No concurrent operation reuses this storage.
        let ok = unsafe {
            if writing {
                WriteFile(
                    handle,
                    buffer.as_ptr().cast(),
                    buffer.len() as u32,
                    &mut transferred,
                    &mut ov,
                )
            } else {
                ReadFile(
                    handle,
                    buffer.as_mut_ptr().cast(),
                    buffer.len() as u32,
                    &mut transferred,
                    &mut ov,
                )
            }
        };
        if ok != 0 {
            return Ok(transferred as usize);
        }
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(ERROR_IO_PENDING) {
            return Err(error);
        }
        let milliseconds = left
            .as_millis()
            .saturating_add(1)
            .min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: the event is owned above and remains live through this wait.
        let wait = unsafe { WaitForSingleObject(event.as_raw_handle(), milliseconds) };
        if wait != 0 {
            let error = if wait == WAIT_TIMEOUT {
                timeout_error()
            } else {
                io::Error::last_os_error()
            };
            // CancelIoEx requests cancellation; completion must be drained before
            // either the buffer or OVERLAPPED can leave scope (Win32 contract).
            unsafe {
                CancelIoEx(handle, &mut ov);
                GetOverlappedResult(handle, &mut ov, &mut transferred, 1);
            }
            return Err(error);
        }
        // SAFETY: the event signalled completion of this exact operation.
        if unsafe { GetOverlappedResult(handle, &mut ov, &mut transferred, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(transferred as usize)
    }

    pub(crate) fn send(name: &str, line: &str, timeout: Duration) -> io::Result<String> {
        if line.contains(['\r', '\n']) || line.len() >= MAX_FRAME || timeout.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "one command line and a positive timeout are required (maximum 1 MiB)",
            ));
        }
        let start = Instant::now();
        let file = open(name, start, timeout)?;
        let mut request = format!("{}\n", line.trim()).into_bytes();
        let mut written = 0;
        while written < request.len() {
            let n = transfer(&file, &mut request[written..], true, start, timeout)?;
            if n == 0 {
                return Err(io::ErrorKind::WriteZero.into());
            }
            written += n;
        }
        let mut response = Vec::new();
        loop {
            let mut buffer = [0_u8; 4096];
            let n = transfer(&file, &mut buffer, false, start, timeout)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "control pipe closed before a complete reply",
                ));
            }
            let end = buffer[..n].iter().position(|b| *b == b'\n');
            response.extend_from_slice(&buffer[..end.unwrap_or(n)]);
            if response.len() > MAX_FRAME {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "control reply exceeds 1 MiB",
                ));
            }
            if end.is_some() {
                if response.last() == Some(&b'\r') {
                    response.pop();
                }
                return String::from_utf8(response)
                    .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error));
            }
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Write};
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn deadline_covers_missing_server_and_invalid_requests() {
        let name = format!("vrdummy-missing-{}", std::process::id());
        let start = Instant::now();
        assert_eq!(
            send(&name, "status", Duration::from_millis(60))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(
            send(&name, "status\nquit", Duration::from_secs(1))
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }

    #[test]
    fn deadline_cancels_idle_partial_reply_and_blocked_write() {
        for (index, read_request, reply) in [(0, true, ""), (1, true, "ok partial"), (2, false, "")]
        {
            let name = format!("vrdummy-deadline-{}-{index}", std::process::id());
            let server_name = name.clone();
            let (release, hold) = mpsc::channel();
            let server = std::thread::spawn(move || {
                let mut file = crate::win32::accept_named_pipe(&server_name).expect("accept");
                if read_request {
                    let mut request = String::new();
                    BufReader::new(file.try_clone().expect("clone"))
                        .read_line(&mut request)
                        .expect("request");
                    file.write_all(reply.as_bytes()).expect("partial reply");
                }
                let _ = hold.recv_timeout(Duration::from_secs(5));
            });
            let request = if read_request {
                "status".to_owned()
            } else {
                "x".repeat(128 * 1024)
            };
            let start = Instant::now();
            let result = send(&name, &request, Duration::from_millis(200));
            let elapsed = start.elapsed();
            release.send(()).expect("release server");
            server.join().expect("server");
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
            assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
        }
    }

    #[test]
    fn incomplete_reply_is_not_reported_as_success() {
        let name = format!("vrdummy-eof-{}", std::process::id());
        let server_name = name.clone();
        let server = std::thread::spawn(move || {
            let mut file = crate::win32::accept_named_pipe(&server_name).expect("accept");
            let mut request = String::new();
            BufReader::new(file.try_clone().expect("clone"))
                .read_line(&mut request)
                .expect("request");
            file.write_all(b"ok").expect("reply without newline");
        });
        let result = send(&name, "status", Duration::from_secs(2));
        server.join().expect("server");
        assert!(result.is_err());
    }
}

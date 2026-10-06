//! Minimal Win32 declarations used by the control pipe server.
//!
//! Declared by hand to avoid a bindings crate. Only the functions listed here
//! are used; signatures follow the Windows SDK.

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use std::ffi::c_void;

/// Win32 `HANDLE`.
pub type HANDLE = *mut c_void;
/// Win32 `BOOL`.
pub type BOOL = i32;
/// Win32 `DWORD`.
pub type DWORD = u32;

/// `INVALID_HANDLE_VALUE`.
pub const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
/// `PIPE_ACCESS_DUPLEX`.
pub const PIPE_ACCESS_DUPLEX: DWORD = 0x0000_0003;
/// `PIPE_TYPE_BYTE`.
pub const PIPE_TYPE_BYTE: DWORD = 0x0000_0000;
/// `PIPE_READMODE_BYTE`.
pub const PIPE_READMODE_BYTE: DWORD = 0x0000_0000;
/// `PIPE_WAIT`.
pub const PIPE_WAIT: DWORD = 0x0000_0000;
/// `PIPE_REJECT_REMOTE_CLIENTS`.
pub const PIPE_REJECT_REMOTE_CLIENTS: DWORD = 0x0000_0008;
/// `ERROR_PIPE_CONNECTED`.
pub const ERROR_PIPE_CONNECTED: DWORD = 535;
/// `ERROR_FILE_NOT_FOUND`.
pub const ERROR_FILE_NOT_FOUND: DWORD = 2;
/// `ERROR_PIPE_BUSY`.
pub const ERROR_PIPE_BUSY: DWORD = 231;

#[cfg(windows)]
#[link(name = "kernel32")]
unsafe extern "system" {
    /// `CreateNamedPipeW`.
    pub fn CreateNamedPipeW(
        name: *const u16,
        open_mode: DWORD,
        pipe_mode: DWORD,
        max_instances: DWORD,
        out_buffer_size: DWORD,
        in_buffer_size: DWORD,
        default_timeout: DWORD,
        security_attributes: *mut c_void,
    ) -> HANDLE;
    /// `ConnectNamedPipe`.
    pub fn ConnectNamedPipe(pipe: HANDLE, overlapped: *mut c_void) -> BOOL;
    /// `WaitNamedPipeW`.
    pub fn WaitNamedPipeW(name: *const u16, timeout_ms: DWORD) -> BOOL;
    /// `DisconnectNamedPipe`.
    pub fn DisconnectNamedPipe(pipe: HANDLE) -> BOOL;
    /// `FlushFileBuffers`.
    pub fn FlushFileBuffers(file: HANDLE) -> BOOL;
    /// `CloseHandle`.
    pub fn CloseHandle(object: HANDLE) -> BOOL;
    /// `GetLastError`.
    pub fn GetLastError() -> DWORD;
}

/// Creates one byte-mode server instance of a local named pipe and blocks
/// until a client connects. The returned file owns the handle.
#[cfg(windows)]
pub fn accept_named_pipe(name: &str) -> std::io::Result<std::fs::File> {
    use std::os::windows::io::FromRawHandle;
    let path = wide(&pipe_path(name));
    // SAFETY: `path` is NUL-terminated and outlives the call; all other
    // arguments are plain values. The returned handle is owned by the File.
    let handle = unsafe {
        CreateNamedPipeW(
            path.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            1,
            65536,
            65536,
            0,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: `handle` is a valid, owned pipe handle returned above.
    let ok = unsafe { ConnectNamedPipe(handle, std::ptr::null_mut()) };
    if ok == 0 {
        // SAFETY: GetLastError has no preconditions.
        let err = unsafe { GetLastError() };
        if err != ERROR_PIPE_CONNECTED {
            // SAFETY: closing the handle we own exactly once.
            unsafe { CloseHandle(handle) };
            return Err(std::io::Error::from_raw_os_error(err as i32));
        }
    }
    // SAFETY: ownership of the handle moves into the File, which closes it.
    Ok(unsafe { std::fs::File::from_raw_handle(handle.cast()) })
}

/// Not available off Windows.
#[cfg(not(windows))]
pub fn accept_named_pipe(_name: &str) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "named pipes are only implemented on Windows",
    ))
}

/// Flushes and disconnects a server-side pipe handle so that the client sees
/// end of stream. The file stays open until dropped.
pub fn disconnect_named_pipe(file: &std::fs::File) {
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        // SAFETY: the handle is valid for the lifetime of `file`.
        unsafe {
            FlushFileBuffers(file.as_raw_handle().cast());
            DisconnectNamedPipe(file.as_raw_handle().cast());
        }
    }
    #[cfg(not(windows))]
    {
        let _ = file;
    }
}

/// Full path of a local named pipe.
pub fn pipe_path(name: &str) -> String {
    format!(r"\\.\pipe\{name}")
}

/// NUL-terminated UTF-16 for Win32 wide-string parameters.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_path_has_prefix() {
        assert_eq!(pipe_path("x"), r"\\.\pipe\x");
    }

    #[test]
    fn wide_is_nul_terminated() {
        assert_eq!(wide("ab"), vec![97, 98, 0]);
    }
}

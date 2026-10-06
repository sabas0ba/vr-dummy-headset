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

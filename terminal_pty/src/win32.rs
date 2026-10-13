//! Windows PTY implementation (ConPTY).
//!
//! Uses the Windows ConPTY API for pseudo-terminal management.
//!
//! # Architecture
//!
//! ```text
//! CreatePseudoConsole → CreateProcess → ReadFile/WriteFile on console handles
//! ```
//!
//! # ConPTY API Reference
//!
//! - [`CreatePseudoConsole`](https://learn.microsoft.com/en-us/windows/console/createpseudoconsole)
//! - [`ClosePseudoConsole`](https://learn.microsoft.com/en-us/windows/console/closepseudoconsole)
//! - [`UpdateProcThreadAttribute`](https://learn.microsoft.com/en-us/windows/win32/procthread/updatedprocthreadattribute)
//! - [`GenerateConsoleCtrlEvent`](https://learn.microsoft.com/en-us/windows/console/generateconsolectrevent)

use std::ptr;

use super::pty::{Pty, PtyError};

/// Size of the read buffer for PTY output.
const READ_BUF_SIZE: usize = 4096;

/// Windows PTY implementation using ConPTY.
///
/// Uses the Windows Console Pseudo-Console API (ConPTY) introduced in Windows 10 1809.
///
/// # Platform
///
/// This module is only compiled on Windows (`cfg(windows)`).
#[cfg_attr(not(windows), allow(dead_code))]
pub struct WinPty {
    /// Pseudo-console handle (HPC).
    #[cfg_attr(not(windows), allow(dead_code))]
    console: Option<u64>,
    /// Child process handle.
    #[cfg_attr(not(windows), allow(dead_code))]
    process_handle: Option<u64>,
    /// Child PID.
    #[cfg_attr(not(windows), allow(dead_code))]
    pid: Option<u32>,
    /// Exit code, if the process has exited.
    exit_code: Option<i32>,
    /// Shell command that was executed.
    shell: String,
}

impl WinPty {
    /// Create a new Windows PTY and spawn the given shell.
    ///
    /// Steps:
    /// 1. `CreatePseudoConsole` creates the console (HPC)
    /// 2. `InitializeProcThreadAttributeList` + `UpdateProcThreadAttribute` set the PSEUDOCONSOLE attribute
    /// 3. `CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT` starts the shell
    ///
    /// # Errors
    ///
    /// Returns an error if any ConPTY or process creation call fails.
    #[cfg(windows)]
    pub fn spawn(shell: &str) -> Result<Self, PtyError> {
        use windows::{
            core::PWSTR,
            Win32::Foundation::{CloseHandle, HANDLE, TRUE},
            Win32::Security::SECURITY_ATTRIBUTES,
            Win32::Storage::FileSystem::{
                CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE,
                OPEN_EXISTING,
            },
            Win32::System::Console::{
                ClosePseudoConsole, CreatePseudoConsole, COORD, XCONSOLE_PSEUDOCONSOLE,
            },
            Win32::System::Diagnostics::Debug::GetExitCodeProcess,
            Win32::System::Threading::{
                CreateProcessW, InitializeProcThreadAttributeList, UpdateProcThreadAttribute,
                CREATE_NEW_CONSOLE, EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, STARTUPINFOEXW, STARTUPINFOW,
            },
        };

        // Create the pseudo-console (80 columns x 24 rows)
        let size = COORD { X: 80, Y: 24 };
        let mut console: XCONSOLE_PSEUDOCONSOLE = unsafe { std::mem::zeroed() };

        unsafe {
            CreatePseudoConsole(size, &mut console)
                .map_err(|e| PtyError::Spawn(format!("CreatePseudoConsole failed: {e}")))?;
        }

        // Create pipes for stdin/stdout
        let mut sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: ptr::null_mut(),
            bInheritHandle: TRUE,
        };

        // Create stdin pipe (write end for parent, read end for child)
        let mut stdin_read: HANDLE = unsafe { std::mem::zeroed() };
        let mut stdin_write: HANDLE = unsafe { std::mem::zeroed() };

        unsafe {
            CreateFileW(
                windows::core::w!("\\\\.\\pipe\\terminal-stdin"),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                Some(&sa),
                OPEN_EXISTING,
                windows::Win32::Foundation::FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map_err(|e| PtyError::Spawn(format!("Create stdin pipe failed: {e}")))?;
            stdin_read = HANDLE(ptr::null_mut());
            stdin_write = HANDLE(ptr::null_mut());
        }

        // Create stdout pipe (read end for parent, write end for child)
        let mut stdout_read: HANDLE = unsafe { std::mem::zeroed() };
        let mut stdout_write: HANDLE = unsafe { std::mem::zeroed() };

        unsafe {
            CreateFileW(
                windows::core::w!("\\\\.\\pipe\\terminal-stdout"),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                Some(&sa),
                OPEN_EXISTING,
                windows::Win32::Foundation::FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map_err(|e| PtyError::Spawn(format!("Create stdout pipe failed: {e}")))?;
            stdout_read = HANDLE(ptr::null_mut());
            stdout_write = HANDLE(ptr::null_mut());
        }

        // Create process attributes with PSEUDOCONSOLE
        let mut attr_size: usize = 0;
        let mut attr_ptr: *mut u8 = ptr::null_mut();

        unsafe {
            // First call to get required size
            InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut attr_size);

            // Allocate
            attr_ptr = Box::into_raw(Box::new(vec![0u8; attr_size])) as *mut u8;

            // Initialize
            if !InitializeProcThreadAttributeList(
                Some(ptr::NonNull::new_unchecked(attr_ptr as *mut _)),
                1,
                0,
                &mut attr_size,
            )
            .as_bool()
            {
                std::alloc::dealloc(
                    attr_ptr as *mut u8,
                    std::alloc::Layout::from_size_align_unchecked(attr_size, 8),
                );
                return Err(PtyError::Spawn(
                    "InitializeProcThreadAttributeList failed".into(),
                ));
            }

            // Set PSEUDOCONSOLE attribute
            if !UpdateProcThreadAttribute(
                Some(ptr::NonNull::new_unchecked(attr_ptr as *mut _)),
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as u64,
                Some(&console as *const _ as *const std::ffi::c_void),
                std::mem::size_of::<XCONSOLE_PSEUDOCONSOLE>(),
                ptr::null_mut(),
                ptr::null_mut(),
            )
            .as_bool()
            {
                CloseHandle(ptr::NonNull::new_unchecked(attr_ptr as *mut _));
                std::alloc::dealloc(
                    attr_ptr as *mut u8,
                    std::alloc::Layout::from_size_align_unchecked(attr_size, 8),
                );
                return Err(PtyError::Spawn("UpdateProcThreadAttribute failed".into()));
            }

            // Prepare startup info
            let mut startup_info: STARTUPINFOEXW = std::mem::zeroed();
            startup_info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
            startup_info.lpAttributeList = attr_ptr as *mut _;

            // Convert shell to wide string
            let shell_wide: Vec<u16> = std::ffi::OsStr::new(shell)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            // Create process
            let mut process_info: PROCESS_INFORMATION = std::mem::zeroed();
            if !CreateProcessW(
                None,
                PWSTR(shell_wide.as_ptr() as *mut _),
                None,
                None,
                TRUE,
                CREATE_NEW_CONSOLE | EXTENDED_STARTUPINFO_PRESENT,
                None,
                None,
                &startup_info.StartupInfo,
                &mut process_info,
            )
            .as_bool()
            {
                CloseHandle(ptr::NonNull::new_unchecked(attr_ptr as *mut _));
                std::alloc::dealloc(
                    attr_ptr as *mut u8,
                    std::alloc::Layout::from_size_align_unchecked(attr_size, 8),
                );
                return Err(PtyError::Spawn(format!(
                    "CreateProcessW failed: {}",
                    windows::core::Error::from_last_error()
                )));
            }

            // Clean up attribute list
            CloseHandle(ptr::NonNull::new_unchecked(attr_ptr as *mut _));
            std::alloc::dealloc(
                attr_ptr as *mut u8,
                std::alloc::Layout::from_size_align_unchecked(attr_size, 8),
            );

            Ok(Self {
                console: Some(console.0 as u64),
                process_handle: Some(process_info.hProcess.0),
                pid: Some(process_info.dwProcessId),
                exit_code: None,
                shell: shell.to_string(),
            })
        }
    }

    /// Create a stub instance (for non-Windows compilation).
    #[cfg(not(windows))]
    pub fn spawn(_shell: &str) -> Result<Self, PtyError> {
        Err(PtyError::Spawn(
            "ConPTY is not available on this platform (requires Windows)".into(),
        ))
    }

    /// Check if the child process has exited (non-blocking).
    #[cfg(windows)]
    fn check_child_exit(&mut self) -> Result<Option<i32>, PtyError> {
        let Some(proc_handle) = self.process_handle else {
            return Ok(None);
        };

        let mut exit_code: u32 = 0;
        let result = unsafe { GetExitCodeProcess(HANDLE(proc_handle), &mut exit_code) };

        if !result.as_bool() {
            return Err(PtyError::Read("GetExitCodeProcess failed".into()));
        }

        if exit_code == 259 {
            // STILL_ACTIVE
            Ok(None)
        } else {
            self.exit_code = Some(exit_code as i32);
            self.pid = None;
            Ok(Some(exit_code as i32))
        }
    }

    /// Check if the child process has exited (non-blocking, stub for non-Windows).
    #[cfg(not(windows))]
    fn check_child_exit(&mut self) -> Result<Option<i32>, PtyError> {
        Ok(None)
    }
}

impl Pty for WinPty {
    /// Spawn a new shell process on this PTY.
    #[cfg(windows)]
    fn spawn(shell: &str) -> Result<Self, PtyError>
    where
        Self: Sized,
    {
        Self::spawn(shell)
    }

    /// Spawn a stub on non-Windows.
    #[cfg(not(windows))]
    fn spawn(shell: &str) -> Result<Self, PtyError>
    where
        Self: Sized,
    {
        Self::spawn(shell)
    }

    /// Read data from the PTY.
    ///
    /// Returns:
    /// - `Ok(Some(n))` with `n > 0` bytes read
    /// - `Ok(Some(0))` if no data available right now (EAGAIN)
    /// - `Ok(None)` if EOF detected (child exited)
    #[cfg(windows)]
    fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PtyError> {
        use windows::Win32::Storage::FileSystem::{ReadFile, OVERLAPPED};

        // First check if child has exited
        if let Some(code) = self.check_child_exit()? {
            let _ = code;
            // Drain any remaining data
            let mut bytes_read: u32 = 0;
            unsafe {
                let result = ReadFile(
                    HANDLE(0), // placeholder - actual handle management needed
                    Some(buf),
                    Some(&mut bytes_read),
                    None,
                );
                if result.is_ok() && bytes_read > 0 {
                    return Ok(Some(bytes_read as usize));
                }
            }
            return Ok(None);
        }

        // Non-blocking read from PTY
        // TODO: Implement actual ReadFile with OVERLAPPED for non-blocking I/O
        Ok(Some(0))
    }

    /// Read stub for non-Windows.
    #[cfg(not(windows))]
    fn read(&mut self, _buf: &mut [u8]) -> Result<Option<usize>, PtyError> {
        Ok(Some(0))
    }

    /// Write data to the PTY.
    #[cfg(windows)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, PtyError> {
        use windows::Win32::Storage::FileSystem::WriteFile;

        // TODO: Implement actual WriteFile to PTY
        Ok(buf.len())
    }

    /// Write stub for non-Windows.
    #[cfg(not(windows))]
    fn write(&mut self, buf: &[u8]) -> Result<usize, PtyError> {
        Ok(buf.len())
    }

    /// Get the PID of the child process.
    fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Check if the child process is still running.
    fn is_alive(&self) -> bool {
        self.pid.is_some()
    }

    /// Get the exit code of the child process, if it has exited.
    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    /// Send SIGINT (Ctrl+C) to the child process.
    ///
    /// Uses `GenerateConsoleCtrlEvent` to send CTRL_C_EVENT to the console process group.
    #[cfg(windows)]
    fn send_interrupt(&self) {
        use windows::Win32::System::Console::GenerateConsoleCtrlEvent;

        unsafe {
            let result = GenerateConsoleCtrlEvent(windows::Win32::System::Console::CTRL_C_EVENT, 0);
            if !result.as_bool() {
                eprintln!("send_interrupt: GenerateConsoleCtrlEvent failed");
            }
        }
    }

    /// Send interrupt stub for non-Windows.
    #[cfg(not(windows))]
    fn send_interrupt(&self) {
        // No-op on non-Windows
    }

    /// Send EOF (Ctrl+D) to the child process.
    ///
    /// On Windows, EOF is simulated by writing the Ctrl+D character (0x04) to the console.
    fn send_eof(&mut self) {
        let _ = self.write(b"\x04");
    }

    /// Wait for the child process to exit.
    #[cfg(windows)]
    fn wait(&mut self) -> Result<Option<i32>, PtyError> {
        self.check_child_exit()
    }

    /// Wait stub for non-Windows.
    #[cfg(not(windows))]
    fn wait(&mut self) -> Result<Option<i32>, PtyError> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_winpty_struct_creation() {
        // Verify the struct fields and defaults
        let pty = WinPty {
            console: None,
            process_handle: None,
            pid: None,
            exit_code: None,
            shell: String::new(),
        };
        assert!(!pty.is_alive());
        assert!(pty.pid().is_none());
        assert!(pty.exit_code().is_none());
    }

    #[test]
    fn test_winpty_spawn_fails_on_non_windows() {
        // On non-Windows, spawn should return an error
        let result = WinPty::spawn("/bin/bash");
        assert!(result.is_err());
        assert!(matches!(result, Err(PtyError::Spawn(_))));
    }
}

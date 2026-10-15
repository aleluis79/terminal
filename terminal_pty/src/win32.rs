//! Windows PTY implementation (ConPTY).
//!
//! Uses the Windows ConPTY API for pseudo-terminal management.
//!
//! # Architecture
//!
//! ```text
//! CreatePipe → CreatePseudoConsole → CreateProcess → ReadFile/WriteFile
//! ```

use std::ptr;

use super::pty::{Pty, PtyError};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Console::HPCON;
use windows::Win32::System::Threading::LPPROC_THREAD_ATTRIBUTE_LIST;

/// Size of the read buffer for PTY output.
#[allow(dead_code)]
const READ_BUF_SIZE: usize = 4096;

/// Windows PTY implementation using `ConPTY`.
#[cfg_attr(not(windows), allow(dead_code))]
pub struct WinPty {
    hpc: windows::Win32::System::Console::HPCON,
    input_write: windows::Win32::Foundation::HANDLE,
    output_read: windows::Win32::Foundation::HANDLE,
    process: windows::Win32::Foundation::HANDLE,
    pid: Option<u32>,
    exit_code: Option<i32>,
    #[allow(dead_code)]
    program: String,
}

unsafe impl Send for WinPty {}

impl WinPty {
    /// Create the two anonymous pipes used to talk to the `ConPTY`.
    ///
    /// Returns `(input_read, input_write, output_read, output_write)` where
    /// `input_write` and `output_read` are kept by the caller and the other
    /// two ends are handed to [`CreatePseudoConsole`].
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Spawn`] if either [`CreatePipe`] call fails.
    #[cfg(windows)]
    fn create_pipes() -> Result<(HANDLE, HANDLE, HANDLE, HANDLE), PtyError> {
        use windows::Win32::Foundation::INVALID_HANDLE_VALUE;
        use windows::Win32::System::Pipes::CreatePipe;

        let mut input_read: HANDLE = INVALID_HANDLE_VALUE;
        let mut input_write: HANDLE = INVALID_HANDLE_VALUE;
        let mut output_read: HANDLE = INVALID_HANDLE_VALUE;
        let mut output_write: HANDLE = INVALID_HANDLE_VALUE;

        unsafe {
            CreatePipe(&raw mut input_read, &raw mut input_write, None, 0)
                .map_err(|e| PtyError::Spawn(format!("CreatePipe (input) failed: {e}")))?;
            CreatePipe(&raw mut output_read, &raw mut output_write, None, 0)
                .map_err(|e| PtyError::Spawn(format!("CreatePipe (output) failed: {e}")))?;
        }
        Ok((input_read, input_write, output_read, output_write))
    }

    /// Build the process thread attribute list that carries the `HPCON`.
    ///
    /// The returned buffer keeps the attribute list memory alive; the caller
    /// must call [`DeleteProcThreadAttributeList`] when done with it.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Spawn`] if the list cannot be initialised or the
    /// pseudo-console attribute cannot be set.
    #[cfg(windows)]
    fn create_attr_list(hpc: HPCON) -> Result<(LPPROC_THREAD_ATTRIBUTE_LIST, Vec<u8>), PtyError> {
        use windows::Win32::System::Threading::{
            DeleteProcThreadAttributeList, InitializeProcThreadAttributeList,
            UpdateProcThreadAttribute, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE,
        };

        let mut attr_size: usize = 0;
        unsafe {
            let _ = InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST(ptr::null_mut()),
                1,
                0,
                &raw mut attr_size,
            );
        }

        let mut attr_mem: Vec<u8> = vec![0; attr_size];
        let attr_list_ptr = LPPROC_THREAD_ATTRIBUTE_LIST(attr_mem.as_mut_ptr().cast());

        unsafe {
            if InitializeProcThreadAttributeList(attr_list_ptr, 1, 0, &raw mut attr_size).is_err() {
                return Err(PtyError::Spawn(
                    "InitializeProcThreadAttributeList failed".to_string(),
                ));
            }

            // NOTE: `lpValue` for `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` is the
            // handle *value* itself, not a pointer to it. Passing `&hpc` makes
            // the child receive a bogus handle and abort with `0xc0000142`.
            let hpc_value = hpc.0 as *const std::ffi::c_void;
            if UpdateProcThreadAttribute(
                attr_list_ptr,
                0,
                PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
                Some(hpc_value),
                std::mem::size_of::<HPCON>(),
                None,
                None,
            )
            .is_err()
            {
                DeleteProcThreadAttributeList(attr_list_ptr);
                return Err(PtyError::Spawn(
                    "UpdateProcThreadAttribute failed".to_string(),
                ));
            }
        }
        Ok((attr_list_ptr, attr_mem))
    }

    /// Spawn a new Windows PTY with `ConPTY`.
    ///
    /// Steps:
    /// 1. `CreatePipe` creates the communication channels
    /// 2. `CreatePseudoConsole` creates the console (`HPCON`)
    /// 3. `InitializeProcThreadAttributeList` + `UpdateProcThreadAttribute`
    ///    set the `PSEUDOCONSOLE` attribute
    /// 4. `CreateProcessW` with `EXTENDED_STARTUPINFO_PRESENT` starts the shell
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Spawn`] if any `ConPTY` or process creation call fails.
    #[cfg(windows)]
    pub fn spawn(program: &str, args: &[String]) -> Result<Self, PtyError> {
        use windows::core::PWSTR;
        use windows::Win32::Foundation::{CloseHandle, FALSE, INVALID_HANDLE_VALUE};
        use windows::Win32::System::Console::{ClosePseudoConsole, CreatePseudoConsole, COORD};
        use windows::Win32::System::Threading::{
            CreateProcessW, DeleteProcThreadAttributeList, CREATE_UNICODE_ENVIRONMENT,
            EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION, STARTF_USESTDHANDLES,
            STARTUPINFOEXW,
        };

        let (input_read, input_write, output_read, output_write) = Self::create_pipes()?;

        let hpc = unsafe {
            match CreatePseudoConsole(COORD { X: 80, Y: 24 }, input_read, output_write, 0) {
                Ok(h) => h,
                Err(e) => {
                    Self::close_handles(&[input_read, input_write, output_read, output_write]);
                    return Err(PtyError::Spawn(format!("CreatePseudoConsole failed: {e}")));
                }
            }
        };

        let (attr_list_ptr, attr_mem) = match Self::create_attr_list(hpc) {
            Ok(v) => v,
            Err(e) => {
                unsafe { ClosePseudoConsole(hpc) };
                Self::close_handles(&[input_read, input_write, output_read, output_write]);
                return Err(e);
            }
        };

        let mut startup_info: STARTUPINFOEXW = unsafe { std::mem::zeroed() };
        startup_info.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;
        startup_info.lpAttributeList = attr_list_ptr;
        // Give the child explicitly invalid std handles. Without this the child
        // inherits our own std handles, which can leave it attached to the
        // parent's console and abort startup with `STATUS_DLL_INIT_FAILED`
        // (`0xc0000142`). The pseudoconsole attribute below is what actually
        // wires the child's console up.
        startup_info.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup_info.StartupInfo.hStdInput = INVALID_HANDLE_VALUE;
        startup_info.StartupInfo.hStdOutput = INVALID_HANDLE_VALUE;
        startup_info.StartupInfo.hStdError = INVALID_HANDLE_VALUE;

        let mut command_line: Vec<u16> = build_command_line(program, args)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let mut process_info: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        let res = unsafe {
            CreateProcessW(
                None,
                PWSTR(command_line.as_mut_ptr()),
                None,
                None,
                FALSE,
                EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
                None,
                None,
                &raw const startup_info.StartupInfo,
                &raw mut process_info,
            )
        };

        unsafe {
            DeleteProcThreadAttributeList(attr_list_ptr);
        }
        drop(attr_mem);

        if let Err(e) = res {
            unsafe { ClosePseudoConsole(hpc) };
            Self::close_handles(&[input_read, input_write, output_read, output_write]);
            return Err(PtyError::Spawn(format!("CreateProcessW failed: {e}")));
        }

        // Free the ConPTY-side pipe ends so broken-channel detection works.
        Self::close_handles(&[input_read, output_write]);
        // We never use the initial thread handle.
        unsafe {
            let _ = CloseHandle(process_info.hThread);
        }

        Ok(Self {
            hpc,
            input_write,
            output_read,
            process: process_info.hProcess,
            pid: Some(process_info.dwProcessId),
            exit_code: None,
            program: program.to_string(),
        })
    }

    /// Close a batch of handles, ignoring errors (they are already gone).
    #[cfg(windows)]
    fn close_handles(handles: &[HANDLE]) {
        use windows::Win32::Foundation::CloseHandle;
        for h in handles {
            let _ = unsafe { CloseHandle(*h) };
        }
    }

    #[cfg(not(windows))]
    pub fn spawn(_program: &str, _args: &[String]) -> Result<Self, PtyError> {
        Err(PtyError::Spawn(
            "ConPTY is not available on this platform".into(),
        ))
    }

    #[cfg(windows)]
    fn check_child_exit(&mut self) -> Result<Option<i32>, PtyError> {
        use windows::Win32::System::Threading::GetExitCodeProcess;

        let mut exit_code: u32 = 0;
        unsafe {
            match GetExitCodeProcess(self.process, &raw mut exit_code) {
                Ok(()) => {}
                Err(_) => return Err(PtyError::Read("GetExitCodeProcess failed".to_string())),
            }
            if exit_code == 259 {
                Ok(None)
            } else {
                #[allow(clippy::cast_possible_wrap)]
                let code = exit_code as i32;
                self.exit_code = Some(code);
                self.pid = None;
                Ok(Some(code))
            }
        }
    }

    #[cfg(not(windows))]
    fn check_child_exit(&mut self) -> Result<Option<i32>, PtyError> {
        Ok(None)
    }
}

impl Pty for WinPty {
    #[cfg(windows)]
    fn spawn(program: &str, args: &[String]) -> Result<Self, PtyError> {
        Self::spawn(program, args)
    }

    #[cfg(not(windows))]
    fn spawn(program: &str, args: &[String]) -> Result<Self, PtyError> {
        Self::spawn(program, args)
    }

    #[cfg(windows)]
    fn read(&mut self, buf: &mut [u8]) -> Result<Option<usize>, PtyError> {
        use std::cmp::min;
        use windows::Win32::Storage::FileSystem::ReadFile;
        use windows::Win32::System::Pipes::PeekNamedPipe;

        let _ = self.check_child_exit()?;
        let mut avail: u32 = 0;
        unsafe {
            match PeekNamedPipe(self.output_read, None, 0, None, Some(&raw mut avail), None) {
                Ok(()) => {}
                Err(_) => {
                    return Ok(None);
                }
            }
        }
        if avail == 0 {
            if self.pid.is_none() {
                return Ok(None);
            }
            return Ok(Some(0));
        }
        let n = min(avail as usize, buf.len());
        let mut read: u32 = 0;
        unsafe {
            match ReadFile(
                self.output_read,
                Some(&mut buf[..n]),
                Some(&raw mut read),
                None,
            ) {
                Ok(()) => {}
                Err(_) => return Err(PtyError::Read("ReadFile failed".to_string())),
            }
        }
        if read == 0 {
            Ok(Some(0))
        } else {
            Ok(Some(read as usize))
        }
    }

    #[cfg(not(windows))]
    fn read(&mut self, _buf: &mut [u8]) -> Result<Option<usize>, PtyError> {
        Ok(Some(0))
    }

    #[cfg(windows)]
    fn write(&mut self, buf: &[u8]) -> Result<usize, PtyError> {
        use windows::Win32::Storage::FileSystem::WriteFile;

        let mut written: u32 = 0;
        unsafe {
            match WriteFile(self.input_write, Some(buf), Some(&raw mut written), None) {
                Ok(()) => {}
                Err(_) => return Err(PtyError::Write("WriteFile failed".to_string())),
            }
        }
        Ok(written as usize)
    }

    #[cfg(not(windows))]
    fn write(&mut self, buf: &[u8]) -> Result<usize, PtyError> {
        Ok(buf.len())
    }

    fn pid(&self) -> Option<u32> {
        self.pid
    }

    fn is_alive(&self) -> bool {
        self.pid.is_some()
    }

    fn exit_code(&self) -> Option<i32> {
        self.exit_code
    }

    #[cfg(windows)]
    fn send_interrupt(&self) {
        use windows::Win32::Storage::FileSystem::WriteFile;

        let mut written: u32 = 0;
        unsafe {
            let _ = WriteFile(
                self.input_write,
                Some(&[0x03]),
                Some(&raw mut written),
                None,
            );
        }
    }

    #[cfg(not(windows))]
    fn send_interrupt(&self) {}

    fn send_eof(&mut self) {
        let _ = self.write(b"\x04");
    }

    #[cfg(windows)]
    fn resize(&mut self, rows: u16, cols: u16) -> Result<(), PtyError> {
        use windows::Win32::System::Console::{ResizePseudoConsole, COORD};

        unsafe {
            #[allow(clippy::cast_possible_wrap)]
            let x = cols as i16;
            #[allow(clippy::cast_possible_wrap)]
            let y = rows as i16;
            match ResizePseudoConsole(self.hpc, COORD { X: x, Y: y }) {
                Ok(()) => Ok(()),
                Err(e) => Err(PtyError::Resize(format!("ResizePseudoConsole failed: {e}"))),
            }
        }
    }

    #[cfg(windows)]
    fn wait(&mut self) -> Result<Option<i32>, PtyError> {
        use windows::Win32::System::Threading::INFINITE;
        use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};

        unsafe {
            let _ = WaitForSingleObject(self.process, INFINITE);
            let mut exit_code: u32 = 0;
            match GetExitCodeProcess(self.process, &raw mut exit_code) {
                Ok(()) => {}
                Err(_) => return Err(PtyError::Read("GetExitCodeProcess failed".to_string())),
            }
            #[allow(clippy::cast_possible_wrap)]
            let code = exit_code as i32;
            self.exit_code = Some(code);
            self.pid = None;
            #[allow(clippy::cast_possible_wrap)]
            Ok(Some(exit_code as i32))
        }
    }

    #[cfg(not(windows))]
    fn wait(&mut self) -> Result<Option<i32>, PtyError> {
        Ok(None)
    }
}

impl Drop for WinPty {
    fn drop(&mut self) {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Console::ClosePseudoConsole;

        unsafe {
            ClosePseudoConsole(self.hpc);
            let _ = CloseHandle(self.input_write);
            let _ = CloseHandle(self.output_read);
            let _ = CloseHandle(self.process);
        }
    }
}

/// Build the `CreateProcessW` command line for `program` plus its arguments.
///
/// Arguments are quoted following the rules `CommandLineToArgvW` uses, so an
/// argument containing spaces arrives as a single argument and is never
/// re-interpreted (CL-25).
#[cfg(windows)]
fn build_command_line(program: &str, args: &[String]) -> String {
    let mut line = quote_arg(program);
    for arg in args {
        line.push(' ');
        line.push_str(&quote_arg(arg));
    }
    line
}

/// Quote a single argument for a Windows command line.
///
/// See <https://learn.microsoft.com/en-us/cpp/c-language/parsing-c-command-line-arguments>.
#[cfg(windows)]
fn quote_arg(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }

    let mut quoted = String::with_capacity(arg.len() + 2);
    quoted.push('"');
    let mut backslashes = 0usize;
    for ch in arg.chars() {
        match ch {
            '\\' => backslashes += 1,
            '"' => {
                // 2n+1 backslashes then the quote itself.
                for _ in 0..=backslashes * 2 {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push('"');
            }
            _ => {
                for _ in 0..backslashes {
                    quoted.push('\\');
                }
                backslashes = 0;
                quoted.push(ch);
            }
        }
    }
    for _ in 0..backslashes * 2 {
        quoted.push('\\');
    }
    quoted.push('"');
    quoted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty::Pty;
    use std::time::{Duration, Instant};

    /// Spawn cmd.exe and read its output until `needle` shows up (or timeout).
    /// Convenience helper: build an owned argument list from string literals.
    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    fn spawn_and_read(program: &str, program_args: &[String], needle: &str) -> (WinPty, String) {
        let mut pty = WinPty::spawn(program, program_args).expect("spawn failed");
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buf = Vec::new();
        while Instant::now() < deadline {
            let mut chunk = [0u8; 4096];
            match pty.read(&mut chunk) {
                Ok(Some(n)) => buf.extend_from_slice(&chunk[..n]),
                Ok(None) => break,
                Err(e) => panic!("read failed: {e}"),
            }
            let text = String::from_utf8_lossy(&buf);
            if text.contains(needle) {
                return (pty, text.into_owned());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "needle {needle:?} not found in output: {}",
            String::from_utf8_lossy(&buf)
        );
    }

    #[test]
    fn test_spawn_cmd_reads_output() {
        let (_pty, output) = spawn_and_read(
            "cmd.exe",
            &args(&["/c", "echo", "hello-conpty"]),
            "hello-conpty",
        );
        assert!(output.contains("hello-conpty"));
    }

    #[test]
    fn test_spawn_passes_arguments() {
        // RF-62: the program receives its arguments (`/c` would be ignored if
        // they were dropped, so the echo never runs).
        let (_pty, output) = spawn_and_read(
            "cmd.exe",
            &args(&["/c", "echo", "hello-args"]),
            "hello-args",
        );
        assert!(output.contains("hello-args"));
    }

    #[test]
    fn test_quote_arg_rules() {
        assert_eq!(quote_arg("plain"), "plain");
        assert_eq!(quote_arg("has space"), "\"has space\"");
        assert_eq!(quote_arg(""), "\"\"");
        assert_eq!(quote_arg("say \"hi\""), "\"say \\\"hi\\\"\"");
        assert_eq!(
            quote_arg("ends with backslash\\"),
            "\"ends with backslash\\\\\""
        );
    }

    #[test]
    fn test_build_command_line() {
        assert_eq!(build_command_line("cmd.exe", &[]), "cmd.exe");
        assert_eq!(
            build_command_line("wsl.exe", &args(&["-d", "Ubuntu", "-e", "ls -l"])),
            "wsl.exe -d Ubuntu -e \"ls -l\""
        );
    }

    #[test]
    fn test_spawn_assigns_pid() {
        let pty = WinPty::spawn("cmd.exe", &[]).expect("spawn failed");
        assert!(pty.pid().is_some());
        assert!(pty.is_alive());
        assert!(pty.exit_code().is_none());
    }

    #[test]
    fn test_write_sends_input_to_shell() {
        let (mut pty, _) = spawn_and_read("cmd.exe", &[], ">");
        let written = pty.write(b"echo written-by-test\r").expect("write failed");
        assert!(written > 0);

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buf = Vec::new();
        while Instant::now() < deadline {
            let mut chunk = [0u8; 4096];
            match pty.read(&mut chunk) {
                Ok(Some(n)) => buf.extend_from_slice(&chunk[..n]),
                Ok(None) => break,
                Err(e) => panic!("read failed: {e}"),
            }
            if String::from_utf8_lossy(&buf).contains("written-by-test") {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("echoed input not found: {}", String::from_utf8_lossy(&buf));
    }

    #[test]
    fn test_wait_reports_exit_code() {
        let mut pty = WinPty::spawn("cmd.exe", &args(&["/c", "exit", "7"])).expect("spawn failed");
        let code = pty.wait().expect("wait failed");
        assert_eq!(code, Some(7));
        assert_eq!(pty.exit_code(), Some(7));
        assert!(!pty.is_alive());
    }

    #[test]
    fn test_resize_succeeds() {
        let mut pty = WinPty::spawn("cmd.exe", &[]).expect("spawn failed");
        pty.resize(30, 100).expect("resize failed");
    }

    /// Regression: `ConPTY` treats a raw `BS` (`0x08`) as Ctrl+Backspace and
    /// `cmd.exe` then deletes the whole word. `DEL` (`0x7f`) removes exactly
    /// one character, which is what the Backspace key must send.
    #[test]
    fn test_backspace_deletes_single_character() {
        let (mut pty, _) = spawn_and_read("cmd.exe", &[], ">");

        // Type "echo AB", erase one character, then finish the word. The shell
        // must end up running `echo ACD`; if the delete wiped the whole word
        // instead, `cmd.exe` would run a bare `echo` and "ACD" never appears.
        pty.write(b"echo AB").expect("write failed");
        std::thread::sleep(Duration::from_millis(150));
        pty.write(b"\x7fCD\r").expect("write failed");

        let deadline = Instant::now() + Duration::from_secs(5);
        let mut buf = Vec::new();
        while Instant::now() < deadline {
            let mut chunk = [0u8; 4096];
            match pty.read(&mut chunk) {
                Ok(Some(n)) => buf.extend_from_slice(&chunk[..n]),
                Ok(None) => break,
                Err(e) => panic!("read failed: {e}"),
            }
            if String::from_utf8_lossy(&buf).contains("ACD") {
                return;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!(
            "expected `echo ACD` to run, got: {}",
            String::from_utf8_lossy(&buf)
        );
    }
}

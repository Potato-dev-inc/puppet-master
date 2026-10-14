//! The console: raw mode with VT input and output, the window size, and a background stdin reader.

use std::io::Read;
use std::sync::mpsc::{channel, Receiver};

/// Puts the console in raw mode for as long as it lives and restores it on drop.
pub struct Console {
    #[cfg(windows)]
    saved: [Option<u32>; 2],
}

#[cfg(windows)]
mod sys {
    pub use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetConsoleScreenBufferInfo, GetStdHandle, SetConsoleMode, CONSOLE_SCREEN_BUFFER_INFO,
        ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT, ENABLE_PROCESSED_OUTPUT,
        ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };
}

impl Console {
    #[cfg(windows)]
    pub fn raw() -> Console {
        let mut saved = [None, None];
        // SAFETY: plain Win32 console calls on the process's own standard handles; every out
        // parameter is a local that lives for the call.
        unsafe {
            let stdin = sys::GetStdHandle(sys::STD_INPUT_HANDLE);
            let mut mode = 0u32;
            if sys::GetConsoleMode(stdin, &mut mode) != 0 {
                saved[0] = Some(mode);
                let raw = (mode & !(sys::ENABLE_LINE_INPUT | sys::ENABLE_ECHO_INPUT | sys::ENABLE_PROCESSED_INPUT))
                    | sys::ENABLE_VIRTUAL_TERMINAL_INPUT;
                sys::SetConsoleMode(stdin, raw);
            }
            let stdout = sys::GetStdHandle(sys::STD_OUTPUT_HANDLE);
            if sys::GetConsoleMode(stdout, &mut mode) != 0 {
                saved[1] = Some(mode);
                sys::SetConsoleMode(stdout, mode | sys::ENABLE_PROCESSED_OUTPUT | sys::ENABLE_VIRTUAL_TERMINAL_PROCESSING);
            }
        }
        Console { saved }
    }

    #[cfg(not(windows))]
    pub fn raw() -> Console {
        let _ = std::process::Command::new("stty").args(["raw", "-echo"]).status();
        Console {}
    }

    /// `(columns, rows)` of the visible window. `PM_FAKE_TUI_SIZE=120x30` overrides it (tests without a console).
    #[cfg(windows)]
    pub fn size() -> (usize, usize) {
        if let Some(size) = size_from_env() {
            return size;
        }
        // SAFETY: as above; `info` is a plain struct the call fills in.
        unsafe {
            let stdout = sys::GetStdHandle(sys::STD_OUTPUT_HANDLE);
            let mut info: sys::CONSOLE_SCREEN_BUFFER_INFO = std::mem::zeroed();
            if sys::GetConsoleScreenBufferInfo(stdout, &mut info) != 0 {
                let cols = i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1;
                let rows = i32::from(info.srWindow.Bottom) - i32::from(info.srWindow.Top) + 1;
                if cols > 0 && rows > 0 {
                    return (cols as usize, rows as usize);
                }
            }
        }
        (120, 30)
    }

    #[cfg(not(windows))]
    pub fn size() -> (usize, usize) {
        if let Some(size) = size_from_env() {
            return size;
        }
        let out = std::process::Command::new("stty").arg("size").output().ok();
        let text = out.map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default();
        let mut parts = text.split_whitespace().filter_map(|p| p.parse::<usize>().ok());
        match (parts.next(), parts.next()) {
            (Some(rows), Some(cols)) => (cols, rows),
            _ => (120, 30),
        }
    }
}

fn size_from_env() -> Option<(usize, usize)> {
    let text = std::env::var("PM_FAKE_TUI_SIZE").ok()?;
    let (cols, rows) = text.split_once('x')?;
    Some((cols.trim().parse().ok()?, rows.trim().parse().ok()?))
}

impl Drop for Console {
    fn drop(&mut self) {
        #[cfg(windows)]
        // SAFETY: restores the modes read in `raw`.
        unsafe {
            if let Some(mode) = self.saved[0] {
                sys::SetConsoleMode(sys::GetStdHandle(sys::STD_INPUT_HANDLE), mode);
            }
            if let Some(mode) = self.saved[1] {
                sys::SetConsoleMode(sys::GetStdHandle(sys::STD_OUTPUT_HANDLE), mode);
            }
        }
        #[cfg(not(windows))]
        let _ = std::process::Command::new("stty").arg("sane").status();
    }
}

/// Reads stdin on a background thread; each message is one read's bytes (so a burst written in one
/// go arrives as one message, which is what the paste-burst rule needs).
pub fn spawn_stdin_reader() -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let mut stdin = std::io::stdin().lock();
        let mut buf = [0u8; 4096];
        while let Ok(n) = stdin.read(&mut buf) {
            if n == 0 || tx.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    rx
}

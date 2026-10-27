//! What the OS says about a process: when it started, its command line, and which TCP ports each
//! process listens on. Plain Win32 calls, no child process (nothing can flash, a look costs
//! microseconds). Windows only for now; elsewhere the answers are empty and the card stays hidden.

use std::collections::HashMap;

/// When the OS says `pid` started (milliseconds since 1970) and its full command line.
#[derive(Debug, Default, Clone)]
pub struct Details {
    pub started_ms: Option<u64>,
    pub command: String,
}

/// Milliseconds since 1970 now.
pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[cfg(windows)]
pub use win::{details, listening_ports};

#[cfg(not(windows))]
pub fn details(_pid: u32) -> Details {
    Details::default()
}

#[cfg(not(windows))]
pub fn listening_ports() -> HashMap<u32, Vec<u16>> {
    HashMap::new()
}

#[cfg(windows)]
mod win {
    use super::*;
    use core::ffi::c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct FileTime {
        low: u32,
        high: u32,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetProcessTimes(handle: *mut c_void, created: *mut FileTime, exited: *mut FileTime, kernel: *mut FileTime, user: *mut FileTime) -> i32;
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(handle: *mut c_void, class: u32, info: *mut c_void, len: u32, returned: *mut u32) -> i32;
    }
    #[link(name = "iphlpapi")]
    extern "system" {
        fn GetExtendedTcpTable(table: *mut c_void, size: *mut u32, order: i32, family: u32, class: u32, reserved: u32) -> u32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// `ProcessCommandLineInformation`: a UNICODE_STRING followed by its characters.
    const PROCESS_COMMAND_LINE_INFORMATION: u32 = 60;
    /// 100 ns ticks between 1601-01-01 and 1970-01-01.
    const FILETIME_UNIX_EPOCH: u64 = 116_444_736_000_000_000;

    pub fn details(pid: u32) -> Details {
        // SAFETY: plain Win32 calls on a handle opened here and closed before returning.
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return Details::default();
            }
            let out = Details { started_ms: started_ms(handle), command: command_line(handle) };
            CloseHandle(handle);
            out
        }
    }

    unsafe fn started_ms(handle: *mut c_void) -> Option<u64> {
        let (mut created, mut a, mut b, mut c) = (FileTime::default(), FileTime::default(), FileTime::default(), FileTime::default());
        if GetProcessTimes(handle, &mut created, &mut a, &mut b, &mut c) == 0 {
            return None;
        }
        let ticks = (u64::from(created.high) << 32) | u64::from(created.low);
        ticks.checked_sub(FILETIME_UNIX_EPOCH).map(|t| t / 10_000)
    }

    unsafe fn command_line(handle: *mut c_void) -> String {
        // The answer is a UNICODE_STRING header followed by the characters; ask with room to spare
        // and once more with the size the system reports when that was too little.
        let mut size = 4096u32;
        for _ in 0..2 {
            let mut buf = vec![0u64; (size as usize).div_ceil(8)];
            let mut needed = 0u32;
            let status = NtQueryInformationProcess(handle, PROCESS_COMMAND_LINE_INFORMATION, buf.as_mut_ptr().cast(), size, &mut needed);
            if status >= 0 {
                // UNICODE_STRING { u16 length (bytes), u16 max, <pad on 64-bit>, *mut u16 }
                let length = u16::from_ne_bytes([*(buf.as_ptr() as *const u8), *(buf.as_ptr() as *const u8).add(1)]) as usize;
                let text = *(buf.as_ptr().add(1) as *const *const u16);
                let base = buf.as_ptr() as usize;
                let inside = (text as usize) >= base && (text as usize) + length <= base + buf.len() * 8;
                if text.is_null() || !inside {
                    return String::new();
                }
                return String::from_utf16_lossy(std::slice::from_raw_parts(text, length / 2));
            }
            if needed == 0 || needed <= size || needed > 64 * 1024 {
                break;
            }
            size = needed;
        }
        String::new()
    }

    /// Listening TCP ports per owning pid (IPv4 and IPv6).
    pub fn listening_ports() -> HashMap<u32, Vec<u16>> {
        const AF_INET: u32 = 2;
        const AF_INET6: u32 = 23;
        const TCP_TABLE_OWNER_PID_LISTENER: u32 = 3;
        let mut out: HashMap<u32, Vec<u16>> = HashMap::new();
        for (family, row_size, port_at, pid_at) in [(AF_INET, 24usize, 8usize, 20usize), (AF_INET6, 56, 20, 52)] {
            let mut size = 0u32;
            let mut buf: Vec<u32> = Vec::new();
            let mut ok = false;
            for _ in 0..4 {
                let ptr = if buf.is_empty() { std::ptr::null_mut() } else { buf.as_mut_ptr().cast() };
                // SAFETY: `ptr` is null or a buffer of at least `size` bytes; the call reports the size it needs.
                let code = unsafe { GetExtendedTcpTable(ptr, &mut size, 0, family, TCP_TABLE_OWNER_PID_LISTENER, 0) };
                match code {
                    0 => {
                        ok = true;
                        break;
                    }
                    122 => buf = vec![0u32; (size as usize + 64).div_ceil(4)],
                    _ => break,
                }
            }
            if !ok || buf.is_empty() {
                continue;
            }
            let bytes: &[u8] = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast(), buf.len() * 4) };
            let count = u32::from_ne_bytes(bytes[0..4].try_into().unwrap_or([0; 4])) as usize;
            for i in 0..count {
                let start = 4 + i * row_size;
                let Some(row) = bytes.get(start..start + row_size) else { break };
                let word = |at: usize| u32::from_ne_bytes(row[at..at + 4].try_into().unwrap_or([0; 4]));
                // The port sits in the low 16 bits, in network byte order.
                let port = u16::from_be((word(port_at) & 0xFFFF) as u16);
                let pid = word(pid_at);
                if port != 0 {
                    let list = out.entry(pid).or_default();
                    if !list.contains(&port) {
                        list.push(port);
                    }
                }
            }
        }
        for list in out.values_mut() {
            list.sort_unstable();
        }
        out
    }
}

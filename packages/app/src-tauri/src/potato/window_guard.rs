//! The visible-window guard of the bot's computer. WSLg (the Linux graphics Windows adds to a WSL 2 distro) shows a Linux
//! window as a Windows window, owned by `msrdc` / `msrdcw`, its remote-desktop client. The sandbox's boot hook cuts WSLg
//! off, so no such window may ever appear on the person's screens. This is the check that proves it, run after every
//! start (`Lifecycle::ensure_ready` calls `WindowCheck::visible_remote_window`): if one is visible, the start fails, the
//! sandbox is stopped, and the person gets an error card (`LifecycleError::VisibleWindow`).
//!
//! `Msrdc` is the production `WindowCheck`. It asks PowerShell (hidden, no script file: the script travels as an encoded
//! command) which top-level windows are visible and owned by an `msrdc` process (`EnumWindows`). If it cannot tell
//! (PowerShell missing, no answer, an answer it cannot read) it says "a window is visible": the start is stopped rather
//! than risking a window on a screen. The process start is injected (`Probe`), so the tests use a fake.
//!
//! Lives next to `sandbox_host.rs`, not in `sandbox/`: the one-door guard keeps every other process start out of that folder.

use std::sync::Arc;

use super::sandbox::lifecycle::WindowCheck;

/// The check of the spike (`winlist.ps1`): the visible top-level windows whose process is `msrdc` or `msrdcw`.
/// The last-but-one line is the count the Rust side reads; the rows after it are for a person reading a log.
pub const WINLIST_SCRIPT: &str = r####"
Add-Type @"
using System; using System.Text; using System.Runtime.InteropServices; using System.Collections.Generic;
public class WL {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p, IntPtr l);
  [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int L,T,R,B; }
  public static List<string> All() { var res = new List<string>();
    EnumWindows((h,l)=>{ if(!IsWindowVisible(h)) return true; uint pid; GetWindowThreadProcessId(h,out pid); var sb=new StringBuilder(256); GetWindowText(h,sb,256); RECT r; GetWindowRect(h,out r);
      res.Add(pid+"|"+sb+"|"+r.L+","+r.T+","+r.R+","+r.B); return true; },IntPtr.Zero); return res; } }
"@
$ids = @(Get-Process msrdc,msrdcw -ErrorAction SilentlyContinue | % Id)
$rows = [WL]::All() | ? { $ids -contains [int]($_.Split('|')[0]) }
"msrdc pids: $($ids -join ',')  visible msrdc windows: $(@($rows).Count)"
$rows
"####;

/// How the script is run. The real one starts PowerShell; a test answers with text.
pub trait Probe: Send + Sync {
    /// Runs `script` and gives back what it printed.
    fn run(&self, script: &str) -> Result<String, String>;
}

/// PowerShell, hidden, with the script as an encoded command (nothing is written to disk).
pub struct PowerShell;

impl Probe for PowerShell {
    fn run(&self, script: &str) -> Result<String, String> {
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, utf16);
        let mut command = crate::proc_util::hidden_command("powershell.exe");
        command.args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-EncodedCommand"]).arg(encoded);
        let out = command.output().map_err(|e| e.to_string())?;
        let text = String::from_utf8_lossy(&out.stdout).to_string();
        if !out.status.success() && !text.contains("visible msrdc windows:") {
            return Err(format!("powershell exited with {:?}", out.status.code()));
        }
        Ok(text)
    }
}

/// The count and the report line out of what the script printed.
pub fn parse_report(text: &str) -> Result<(usize, String), String> {
    let line = text.lines().find(|l| l.contains("visible msrdc windows:")).ok_or_else(|| "the window check gave no count".to_string())?;
    let count = line.rsplit(':').next().and_then(|n| n.trim().parse::<usize>().ok()).ok_or_else(|| "the window check gave a count that is not a number".to_string())?;
    Ok((count, line.trim().to_string()))
}

/// How many visible `msrdc` windows there are right now, and the report line.
pub fn visible_windows(probe: &dyn Probe) -> Result<(usize, String), String> {
    parse_report(&probe.run(WINLIST_SCRIPT)?)
}

/// The production `WindowCheck`.
pub struct Msrdc {
    probe: Arc<dyn Probe>,
}

impl Msrdc {
    /// The real check (PowerShell).
    pub fn live() -> Self {
        Self { probe: Arc::new(PowerShell) }
    }

    /// A check with another way to run the script (tests).
    #[cfg(test)]
    pub fn with_probe(probe: Arc<dyn Probe>) -> Self {
        Self { probe }
    }
}

impl WindowCheck for Msrdc {
    fn visible_remote_window(&self) -> bool {
        match visible_windows(self.probe.as_ref()) {
            Ok((count, _)) => count > 0,
            // Cannot tell: the start is stopped.
            Err(_) => true,
        }
    }
}

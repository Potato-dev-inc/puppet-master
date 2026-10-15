# Lists every visible top-level window of the dev app (puppet_master_app.exe) with its
# position, and exits 1 if any of them sits on the primary monitor. Used to prove live
# tests keep Puppet Master windows on the secondary monitor (see tauri.dev.local.json).
Add-Type @'
using System; using System.Runtime.InteropServices; using System.Text; using System.Collections.Generic;
public class PmWin {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  public struct RECT { public int Left, Top, Right, Bottom; }
  public static List<IntPtr> All() { var l = new List<IntPtr>(); EnumWindows((h, p) => { l.Add(h); return true; }, IntPtr.Zero); return l; }
}
'@
Add-Type -AssemblyName System.Windows.Forms
$primary = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
$pids = @(Get-Process puppet_master_app -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
$fg = [PmWin]::GetForegroundWindow()
$bad = 0
foreach ($h in [PmWin]::All()) {
  if (-not [PmWin]::IsWindowVisible($h)) { continue }
  [uint32]$procId = 0; [PmWin]::GetWindowThreadProcessId($h, [ref]$procId) | Out-Null
  if ($pids -notcontains [int]$procId) { continue }
  $r = New-Object PmWin+RECT; [PmWin]::GetWindowRect($h, [ref]$r) | Out-Null
  $sb = New-Object System.Text.StringBuilder 256; [PmWin]::GetWindowText($h, $sb, 256) | Out-Null
  if (($r.Right - $r.Left) -lt 100 -or ($r.Bottom - $r.Top) -lt 100) { continue }  # tray/helper stubs
  $cx = [int](($r.Left + $r.Right) / 2); $cy = [int](($r.Top + $r.Bottom) / 2)
  $onPrimary = $primary.Contains($cx, $cy)
  if ($onPrimary) { $bad++ }
  "{0,-48} left={1,6} top={2,5} size={5}x{6} primary={3} foreground={4}" -f $sb.ToString(), $r.Left, $r.Top, $onPrimary, ($h -eq $fg), ($r.Right - $r.Left), ($r.Bottom - $r.Top)
}
if ($pids.Count -eq 0) { 'puppet_master_app is not running'; exit 2 }
if ($bad -gt 0) { "FAIL: $bad Puppet Master window(s) on the primary monitor"; exit 1 }
'OK: no Puppet Master window on the primary monitor'

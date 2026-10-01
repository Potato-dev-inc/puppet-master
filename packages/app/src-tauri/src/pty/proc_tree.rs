//! Terminate a whole process tree for pids this app spawned (never by name).

use std::process::Command;

/// Kill `pid` and every descendant. Best effort; a pid that is already gone is a no-op.
pub fn kill_process_tree(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(unix)]
    {
        let mut order = Vec::new();
        collect_descendants(pid, &mut order);
        // Leaves first so parents cannot respawn/reparent while we walk.
        for child in order.iter().rev() {
            let _ = Command::new("kill").args(["-KILL", &child.to_string()]).output();
        }
        let _ = Command::new("kill").args(["-KILL", &pid.to_string()]).output();
    }
}

#[cfg(unix)]
fn collect_descendants(pid: u32, out: &mut Vec<u32>) {
    let Ok(output) = Command::new("pgrep").args(["-P", &pid.to_string()]).output() else {
        return;
    };
    for child in String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .filter_map(|value| value.parse::<u32>().ok())
    {
        out.push(child);
        collect_descendants(child, out);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};

    /// Spawn a harmless child that itself spawns a grandchild sleeper. Returns (child, grandchild pid).
    pub fn spawn_child_with_grandchild() -> (Child, u32) {
        #[cfg(windows)]
        let mut command = {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-Command",
                "$p = Start-Process ping -ArgumentList '-n','30','127.0.0.1' -PassThru -WindowStyle Hidden; Write-Output $p.Id; Start-Sleep -Seconds 30",
            ]);
            c
        };
        #[cfg(unix)]
        let mut command = {
            let mut c = Command::new("sh");
            c.args(["-c", "sleep 30 & echo $!; wait"]);
            c
        };
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .expect("spawn helper");
        let mut line = String::new();
        BufReader::new(child.stdout.as_mut().expect("stdout"))
            .read_line(&mut line)
            .expect("grandchild pid line");
        let grandchild = line.trim().parse::<u32>().expect("grandchild pid");
        (child, grandchild)
    }

    pub fn pid_alive(pid: u32) -> bool {
        #[cfg(windows)]
        {
            let out = Command::new("tasklist")
                .args(["/FI", &format!("PID eq {pid}"), "/NH"])
                .output()
                .expect("tasklist");
            String::from_utf8_lossy(&out.stdout)
                .split_whitespace()
                .any(|token| token == pid.to_string())
        }
        #[cfg(unix)]
        {
            Command::new("kill")
                .args(["-0", &pid.to_string()])
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        }
    }

    pub fn wait_until_dead(pid: u32) -> bool {
        for _ in 0..50 {
            if !pid_alive(pid) {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn kill_process_tree_terminates_child_and_grandchild() {
        let (mut child, grandchild) = spawn_child_with_grandchild();
        let root = child.id();
        assert!(pid_alive(root) && pid_alive(grandchild));
        kill_process_tree(root);
        let _ = child.wait();
        assert!(wait_until_dead(root), "child still alive");
        assert!(wait_until_dead(grandchild), "grandchild leaked");
    }
}

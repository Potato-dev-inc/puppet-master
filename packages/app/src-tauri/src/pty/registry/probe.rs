//! A throwaway PTY for the canary (`crate::canary`): the CLI is started exactly like a pane (same
//! command line, same environment, same `vt100` emulator) but is never registered, never shown and
//! never announced to any window. The caller drives it and kills it.

use super::*;

pub(crate) struct ProbePty {
    parser: Arc<Mutex<vt100::Parser>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
    _master: Box<dyn MasterPty + Send>,
    pid: u32,
    started: Instant,
}

impl ProbePty {
    /// The visible screen as text.
    pub(crate) fn screen(&self) -> String {
        self.parser.lock().screen().contents()
    }

    pub(crate) fn send(&mut self, keys: &str) -> Result<(), String> {
        self.writer
            .write_all(keys.as_bytes())
            .and_then(|()| self.writer.flush())
            .map_err(|e| e.to_string())
    }

    /// False once the process has ended and its output is drained.
    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    pub(crate) fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// End the CLI and everything it started.
    pub(crate) fn kill(&mut self) {
        if self.pid > 0 {
            crate::pty::proc_tree::kill_process_tree(self.pid);
        }
        let _ = self.child.kill();
    }
}

impl Drop for ProbePty {
    fn drop(&mut self) {
        self.kill();
    }
}

/// Start `profile`'s CLI in `cwd` on a hidden PTY of `rows` x `cols`. The project's instruction
/// files are not touched (a real spawn syncs them; a check must leave the folder alone).
pub(crate) fn spawn_probe(
    profile: &dyn crate::tui::TuiProfile,
    cwd: &str,
    rows: u16,
    cols: u16,
) -> Result<ProbePty, String> {
    let cmd = spawn::launch_command(
        profile,
        &crate::tui::SpawnRequest { cwd, extra_args: &[], cols, rows, read_only: false },
        &crate::tui::LaunchEnv::default(),
    )?;
    let spawn::PtyChild { master, child, pid } = spawn::open_pty_child(cmd, rows, cols)?;
    let writer = master.take_writer().map_err(|e| format!("take_writer: {e}"))?;
    let mut reader = master.try_clone_reader().map_err(|e| format!("clone_reader: {e}"))?;
    let parser = Arc::new(Mutex::new(vt100::Parser::new(rows, cols, 0)));
    {
        let parser = Arc::clone(&parser);
        thread::Builder::new()
            .name("canary-probe-reader".into())
            .spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = reader.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    parser.lock().process(&buf[..n]);
                }
            })
            .map_err(|e| format!("reader thread: {e}"))?;
    }
    Ok(ProbePty { parser, writer, child, _master: master, pid, started: Instant::now() })
}

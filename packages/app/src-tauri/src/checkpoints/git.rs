//! Thin, hardened wrapper around the `git` CLI for the shadow repository.
//!
//! Every call names `--git-dir` and `--work-tree` explicitly and scrubs inherited `GIT_*`
//! variables, so the user's own repository (HEAD, index, branches, config) is never read or
//! written. User and system git config are replaced by an empty file for the same reason.

use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;

use once_cell::sync::OnceCell;

/// Why a git call failed.
#[derive(Debug, Clone)]
pub struct GitError(pub String);

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for GitError {}

/// Whether a usable `git` is on PATH (probed once per process).
pub fn git_available() -> bool {
    static AVAILABLE: OnceCell<bool> = OnceCell::new();
    *AVAILABLE.get_or_init(|| {
        let mut cmd = crate::proc_util::hidden_command("git");
        cmd.arg("--version").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd.status().map(|s| s.success()).unwrap_or(false)
    })
}

/// Prefix of the PTY reader threads' names (`pty/registry/spawn.rs`).
const PTY_READER_THREAD_PREFIX: &str = "pty-reader";

/// Git work must never run on a PTY reader thread: a slow `git` there freezes the pane's output
/// and every status hook. The checkpoint hooks hand their work to a worker thread; this makes a
/// regression loud (a panic in tests, an error line in the app) instead of a silent stall.
fn forbid_pty_reader_thread() {
    let thread = std::thread::current();
    if thread.name().is_some_and(|n| n.starts_with(PTY_READER_THREAD_PREFIX)) {
        tracing::error!(thread = ?thread.name(), "git ran on a PTY reader thread");
        #[cfg(test)]
        panic!("git ran on a PTY reader thread ({:?})", thread.name());
    }
}

/// `path` without the Windows extended-length prefix: `\\?\C:\x` becomes `C:\x` and
/// `\\?\UNC\host\share` becomes `\\host\share`. Any other path comes back unchanged.
pub(super) fn without_verbatim_prefix(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        // Drive-letter paths only: `\\?\Volume{...}` has no plain spelling.
        Some(rest) if rest.as_bytes().get(1) == Some(&b':') => PathBuf::from(rest),
        _ => path.to_path_buf(),
    }
}

/// One shadow repository: a private git dir plus the project folder used as the work tree.
#[derive(Debug, Clone)]
pub struct Shadow {
    pub work_tree: PathBuf,
    pub git_dir: PathBuf,
}

/// Output of a git call.
pub struct Out {
    pub stdout: Vec<u8>,
}

impl Out {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).trim().to_string()
    }
}

impl Shadow {
    pub fn new(work_tree: &Path) -> Self {
        // Git for Windows cannot open `\\?\C:\...` paths: a config file named that way fails with
        // "unknown error occurred while reading the configuration files", so no save point would
        // ever be taken. A folder that went through `canonicalize` has that spelling.
        let work_tree = without_verbatim_prefix(work_tree);
        Self { git_dir: work_tree.join(".puppet-master").join("checkpoints.git"), work_tree }
    }

    /// The same shadow repository (same `git_dir`) with another work tree. Used by the
    /// What-broke-it copies: `checkout-index` then writes into the copy, never into the project.
    #[allow(dead_code)] // first caller: `checkpoints/bisect/copy.rs`
    pub fn with_work_tree(&self, dir: &Path) -> Shadow {
        Shadow { work_tree: dir.to_path_buf(), git_dir: self.git_dir.clone() }
    }

    fn empty_config(&self) -> PathBuf {
        self.git_dir.join("pm-empty-config")
    }

    /// Run git with an optional private index file and optional stdin.
    pub fn run<S: AsRef<OsStr>>(
        &self,
        args: &[S],
        index: Option<&Path>,
        stdin: Option<&[u8]>,
        env: &[(&str, String)],
    ) -> Result<Out, GitError> {
        forbid_pty_reader_thread();
        let mut cmd = crate::proc_util::hidden_command("git");
        cmd.arg("--git-dir").arg(&self.git_dir).arg("--work-tree").arg(&self.work_tree);
        // Leave no room for the user's config (hooks, autocrlf, fsmonitor, safe.directory games).
        cmd.args(["-c", "core.autocrlf=false", "-c", "core.safecrlf=false", "-c", "core.quotepath=false"]);
        cmd.args(["-c", "core.fsmonitor=false", "-c", "core.longpaths=true", "-c", "gc.auto=0"]);
        cmd.args(args);
        cmd.current_dir(&self.work_tree);
        for (key, _) in std::env::vars() {
            if key.starts_with("GIT_") {
                cmd.env_remove(key);
            }
        }
        cmd.env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.empty_config())
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0");
        if let Some(index) = index {
            cmd.env("GIT_INDEX_FILE", index);
        }
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if stdin.is_some() { Stdio::piped() } else { Stdio::null() });
        let mut child = cmd.spawn().map_err(|e| GitError(format!("could not start git: {e}")))?;
        if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
            // Written from a thread so a large stdin cannot deadlock against a full stdout pipe.
            let data = data.to_vec();
            std::thread::spawn(move || {
                let _ = pipe.write_all(&data);
            });
        }
        let output = child.wait_with_output().map_err(|e| GitError(format!("git did not finish: {e}")))?;
        if output.status.success() {
            Ok(Out { stdout: output.stdout })
        } else {
            let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(GitError(if err.is_empty() { format!("git exited with {}", output.status) } else { err }))
        }
    }

    pub fn exists(&self) -> bool {
        self.git_dir.join("HEAD").is_file()
    }

    /// Create the shadow repository if needed and write its private configuration.
    pub fn ensure(&self, extra_excludes: &[String]) -> Result<(), GitError> {
        if !self.exists() {
            crate::git_exclude::ensure_project_storage_excluded(&self.work_tree);
            std::fs::create_dir_all(&self.git_dir).map_err(|e| GitError(format!("create {}: {e}", self.git_dir.display())))?;
            std::fs::write(self.empty_config(), "").map_err(|e| GitError(e.to_string()))?;
            self.run(&["init", "--quiet", "--template="], None, None, &[])?;
            for (k, v) in [
                ("core.bare", "false"),
                ("user.name", "Puppet Master"),
                ("user.email", "checkpoints@puppet-master.invalid"),
                ("gc.pruneExpire", "1.day.ago"),
                ("commit.gpgsign", "false"),
            ] {
                self.run(&["config", k, v], None, None, &[])?;
            }
        } else if !self.empty_config().exists() {
            let _ = std::fs::write(self.empty_config(), "");
        }
        self.write_excludes(extra_excludes)
    }

    /// `info/exclude`: our defaults, then the user's own `.git/info/exclude` when present.
    /// `.gitignore` files in the work tree are honoured by git itself.
    fn write_excludes(&self, extra: &[String]) -> Result<(), GitError> {
        let mut text = String::from("# managed by Puppet Master\n.git/\n.puppet-master/\n");
        for line in DEFAULT_EXCLUDES {
            text.push_str(line);
            text.push('\n');
        }
        for line in extra {
            text.push_str(line);
            text.push('\n');
        }
        if let Ok(user) = std::fs::read_to_string(self.work_tree.join(".git").join("info").join("exclude")) {
            text.push_str("# from the project's .git/info/exclude\n");
            text.push_str(&user);
            text.push('\n');
        }
        let info = self.git_dir.join("info");
        std::fs::create_dir_all(&info).map_err(|e| GitError(e.to_string()))?;
        let path = info.join("exclude");
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            std::fs::write(path, text).map_err(|e| GitError(e.to_string()))?;
        }
        Ok(())
    }
}

/// Heavy, regenerable folders and OS litter. Skipping them keeps a folder without any
/// `.gitignore` (a typical non-git project) from filling the disk. Restore only ever touches
/// files that were snapshotted, so these are never deleted or overwritten.
const DEFAULT_EXCLUDES: &[&str] = &[
    "node_modules/",
    ".venv/",
    "venv/",
    "__pycache__/",
    ".next/",
    ".cache/",
    ".DS_Store",
    "Thumbs.db",
];

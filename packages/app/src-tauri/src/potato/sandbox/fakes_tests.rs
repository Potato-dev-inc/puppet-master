//! Test doubles for the sandbox: `FakeGuest` is a `wsl::Runner` that behaves like a small WSL with one
//! distro, so install, start and stop run end to end with no `wsl.exe` and no Linux.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;

use super::wsl::{Holder, Output, Prepared, Runner, Verb, Wsl, WslError};

pub struct State {
    pub wsl_ok: bool,
    pub installed: Vec<String>,
    pub running: Vec<String>,
    pub boot_done: bool,
    pub boot_hangs: bool,
    pub boot_failed: bool,
    /// `None` = the probe has not run; `Some(true)` passed; `Some(false)` failed.
    pub egress: Option<bool>,
    pub egress_probe_passes: bool,
    pub desktop_up: bool,
    pub control_up: bool,
    pub desktop_starts: bool,
    pub apply_lines: Vec<String>,
    pub apply_code: i32,
    pub install_code: i32,
    pub calls: Vec<Prepared>,
    pub holders: Vec<Arc<AtomicBool>>,
    pub elevated_seen: usize,
}

pub struct FakeGuest {
    pub st: Mutex<State>,
}

pub struct FakeHolder {
    alive: Arc<AtomicBool>,
}

impl Holder for FakeHolder {
    fn alive(&mut self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    fn stop(&mut self) {
        self.alive.store(false, Ordering::SeqCst);
    }
}

fn utf16(text: &str) -> Vec<u8> {
    let mut bytes = vec![0xFF, 0xFE];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    bytes
}

fn ok(stdout: Vec<u8>) -> Result<Output, WslError> {
    Ok(Output { code: Some(0), stdout, stderr: Vec::new() })
}

fn code(c: i32) -> Result<Output, WslError> {
    Ok(Output { code: Some(c), stdout: Vec::new(), stderr: Vec::new() })
}

impl FakeGuest {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            st: Mutex::new(State {
                wsl_ok: true,
                installed: Vec::new(),
                running: Vec::new(),
                boot_done: false,
                boot_hangs: false,
                boot_failed: false,
                egress: None,
                egress_probe_passes: true,
                desktop_up: false,
                control_up: false,
                desktop_starts: true,
                apply_lines: vec![
                    "PROGRESS 1/4 Installing packages".to_string(),
                    "PROGRESS 2/4 Installing Chrome".to_string(),
                    "PROGRESS 4/4 Done".to_string(),
                    "RESULT {}".to_string(),
                ],
                apply_code: 0,
                install_code: 0,
                calls: Vec::new(),
                holders: Vec::new(),
                elevated_seen: 0,
            }),
        })
    }

    pub fn with_installed(name: &str) -> Arc<Self> {
        let guest = Self::new();
        guest.st.lock().installed.push(name.to_string());
        guest
    }

    pub fn wsl(self: &Arc<Self>) -> Wsl {
        Wsl::new(self.clone())
    }

    pub fn calls(&self) -> Vec<Prepared> {
        self.st.lock().calls.clone()
    }

    /// Each call's arguments on one line.
    pub fn joined(&self) -> Vec<String> {
        self.calls().iter().map(|c| c.args.join(" ")).collect()
    }

    pub fn holder_alive(&self) -> bool {
        self.st.lock().holders.iter().any(|h| h.load(Ordering::SeqCst))
    }

    fn readiness_text(st: &State) -> String {
        let egress = match st.egress {
            Some(true) => "egress_state=passed\negress_reason=\n".to_string(),
            Some(false) => "egress_state=failed\negress_reason=gateway reachable\n".to_string(),
            None => "egress_state=\negress_reason=\n".to_string(),
        };
        format!(
            "boot_done={}\nboot_failed={}\n{egress}desktop_up={}\ncontrol_up={}\n",
            u8::from(st.boot_done),
            u8::from(st.boot_failed),
            u8::from(st.desktop_up),
            u8::from(st.control_up)
        )
    }

    fn table(st: &State) -> String {
        let mut text = String::from("  NAME            STATE           VERSION\n");
        text.push_str("* Ubuntu          Stopped         2\n");
        for name in &st.installed {
            let state = if st.running.contains(name) { "Running" } else { "Stopped" };
            text.push_str(&format!("  {name:<15} {state:<15} 2\n"));
        }
        text
    }

    fn guest(&self, st: &mut State, call: &Prepared, on_line: &mut dyn FnMut(&str)) -> Result<Output, WslError> {
        let at = call.args.iter().position(|a| a == "--exec").unwrap_or(0);
        let argv: Vec<&str> = call.args[at + 1..].iter().map(String::as_str).collect();
        let joined = argv.join(" ");
        match argv.as_slice() {
            ["/bin/sh", "-s"] => ok(Self::readiness_text(st).into_bytes()),
            ["/bin/bash", script] if script.ends_with("pm-sbx-egress.sh") => code(0),
            ["/bin/bash", script] if script.ends_with("pm-sbx-egress-probe.sh") => {
                st.egress = Some(st.egress_probe_passes);
                code(if st.egress_probe_passes { 0 } else { 1 })
            }
            ["/bin/sh", "-c", script] if script.contains("pm-sbx-control") => {
                st.control_up = true;
                ok(Vec::new())
            }
            ["/bin/sh", "-c", script] if script.contains("control.token") => ok(Vec::new()),
            ["/usr/local/bin/pm-sbx-start-desktop", "start"] => {
                if st.desktop_starts {
                    st.desktop_up = true;
                    code(0)
                } else {
                    code(1)
                }
            }
            ["/bin/mkdir", ..] | ["/bin/tar", ..] => code(0),
            ["/usr/bin/python3", "-I", script, ..] if script.ends_with("apply-recipe.py") => {
                for line in st.apply_lines.clone() {
                    on_line(&line);
                }
                code(st.apply_code)
            }
            ["/usr/bin/env", "DISPLAY=:99", "/usr/bin/python3", ..] => ok(b"{\"ok\":true}".to_vec()),
            _ => panic!("the fake guest was asked for something it does not know: {joined}"),
        }
    }
}

impl Runner for FakeGuest {
    fn run(&self, call: &Prepared, on_line: &mut dyn FnMut(&str)) -> Result<Output, WslError> {
        let mut st = self.st.lock();
        st.calls.push(call.clone());
        if call.elevated() {
            st.elevated_seen += 1;
        }
        if !st.wsl_ok && !matches!(call.verb, Verb::EnableFeature) {
            return code(1);
        }
        match &call.verb {
            Verb::Status => code(0),
            Verb::List if call.args.iter().any(|a| a == "--running") => {
                let names = st.running.join("\r\n");
                ok(utf16(&names))
            }
            Verb::List => ok(utf16(&Self::table(&st))),
            Verb::EnableFeature => code(3010),
            Verb::Install(name) => {
                if st.install_code != 0 {
                    return code(st.install_code);
                }
                st.installed.push(name.to_string());
                code(0)
            }
            Verb::Terminate(name) => {
                st.running.retain(|n| n != name.as_str());
                st.boot_done = false;
                st.desktop_up = false;
                st.control_up = false;
                st.egress = None;
                code(0)
            }
            Verb::Unregister(name) => {
                st.installed.retain(|n| n != name.as_str());
                st.running.retain(|n| n != name.as_str());
                code(0)
            }
            Verb::Export(_) => code(0),
            Verb::Import(name) => {
                st.installed.push(name.to_string());
                code(0)
            }
            Verb::Guest { name, .. } => {
                if !st.installed.contains(&name.to_string()) {
                    return code(1);
                }
                if !st.running.contains(&name.to_string()) {
                    st.running.push(name.to_string());
                }
                self.guest(&mut st, call, on_line)
            }
        }
    }

    fn spawn(&self, call: &Prepared) -> Result<Box<dyn Holder>, WslError> {
        let mut st = self.st.lock();
        st.calls.push(call.clone());
        let Verb::Guest { name, .. } = &call.verb else { return Err(WslError::Refused("not a guest call")) };
        if !st.installed.contains(&name.to_string()) {
            return Err(WslError::Failed { code: Some(1), detail: "no such distro".to_string() });
        }
        st.running.push(name.to_string());
        if !st.boot_hangs {
            st.boot_done = true;
        }
        let flag = Arc::new(AtomicBool::new(true));
        st.holders.push(flag.clone());
        Ok(Box::new(FakeHolder { alive: flag }))
    }
}

/// A scratch folder under the system temp folder, taken away (file by file) when it goes out of scope.
pub struct Scratch(pub std::path::PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Self {
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!("pm-sbx-{name}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn clear(dir: &std::path::Path) {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    Self::clear(&path);
                } else {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        let _ = std::fs::remove_dir(dir);
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        Self::clear(&self.0);
    }
}

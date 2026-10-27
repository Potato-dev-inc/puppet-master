//! "Start Puppet Master when I sign in" (OFF by default). The OS keeps the switch (a Run entry on
//! Windows, a Launch Agent on macOS) through `tauri-plugin-autostart`. This module only maps that
//! switch to a trait, so the decisions are unit tested with a fake and never touch the real
//! registry.
//!
//! The sign-in entry passes `--background`. A start with that argument opens no window and takes no
//! focus: it only leaves the tray icon and the companion keeper running.

use tauri::{AppHandle, Runtime};
use tauri_plugin_autostart::ManagerExt;

/// The argument the sign-in entry passes to the app.
pub const BACKGROUND_ARG: &str = "--background";

/// The OS switch behind "Start when I sign in".
pub trait SignInSwitch {
    fn is_on(&self) -> Result<bool, String>;
    fn turn_on(&self) -> Result<(), String>;
    fn turn_off(&self) -> Result<(), String>;
}

/// The plugin's own switch. Used only by the app; tests pass a fake `SignInSwitch`.
struct PluginSwitch<'a, R: Runtime>(&'a AppHandle<R>);

impl<R: Runtime> SignInSwitch for PluginSwitch<'_, R> {
    fn is_on(&self) -> Result<bool, String> {
        self.0.autolaunch().is_enabled().map_err(|e| e.to_string())
    }

    fn turn_on(&self) -> Result<(), String> {
        self.0.autolaunch().enable().map_err(|e| e.to_string())
    }

    fn turn_off(&self) -> Result<(), String> {
        self.0.autolaunch().disable().map_err(|e| e.to_string())
    }
}

/// The plugin, with the sign-in argument. Registering it writes nothing; only `set` does.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri_plugin_autostart::Builder::new().arg(BACKGROUND_ARG).build()
}

/// Whether the switch is on right now (read from the OS, never from a saved copy).
pub fn get(app: &AppHandle) -> Result<bool, String> {
    read(&PluginSwitch(app))
}

/// Turn the switch on or off. Returns the state the OS reports afterward, so the UI shows what
/// really happened.
pub fn set(app: &AppHandle, enabled: bool) -> Result<bool, String> {
    write(&PluginSwitch(app), enabled)
}

pub fn read<S: SignInSwitch>(switch: &S) -> Result<bool, String> {
    switch.is_on()
}

pub fn write<S: SignInSwitch>(switch: &S, enabled: bool) -> Result<bool, String> {
    if enabled {
        switch.turn_on()?;
    } else {
        switch.turn_off()?;
    }
    switch.is_on()
}

/// True when this start came from sign-in (the entry passed `--background`).
pub fn is_background_start(args: &[String]) -> bool {
    args.iter().any(|a| a == BACKGROUND_ARG)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    /// A fake OS switch that records each call. `fail` makes every write return an error.
    struct FakeSwitch {
        on: Cell<bool>,
        calls: RefCell<Vec<&'static str>>,
        fail: bool,
        /// Ignore writes, as if the OS refused them without an error.
        stuck: bool,
    }

    impl FakeSwitch {
        fn new(on: bool) -> Self {
            Self { on: Cell::new(on), calls: RefCell::new(Vec::new()), fail: false, stuck: false }
        }
    }

    impl SignInSwitch for FakeSwitch {
        fn is_on(&self) -> Result<bool, String> {
            self.calls.borrow_mut().push("is_on");
            Ok(self.on.get())
        }

        fn turn_on(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("turn_on");
            if self.fail {
                return Err("registry denied".into());
            }
            if !self.stuck {
                self.on.set(true);
            }
            Ok(())
        }

        fn turn_off(&self) -> Result<(), String> {
            self.calls.borrow_mut().push("turn_off");
            if self.fail {
                return Err("registry denied".into());
            }
            if !self.stuck {
                self.on.set(false);
            }
            Ok(())
        }
    }

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_plain_start_is_not_a_sign_in_start() {
        assert!(!is_background_start(&args(&["puppet_master_app.exe"])));
        assert!(!is_background_start(&args(&["puppet_master_app.exe", "--new-pane", "claude"])));
        // Only the exact flag counts; a similar word in a path or value does not.
        assert!(!is_background_start(&args(&["puppet_master_app.exe", "--background-task"])));
    }

    #[test]
    fn the_sign_in_entry_is_recognised_wherever_it_sits() {
        assert!(is_background_start(&args(&["puppet_master_app.exe", "--background"])));
        assert!(is_background_start(&args(&["puppet_master_app.exe", "--new-pane", "x", "--background"])));
    }

    #[test]
    fn reading_the_switch_asks_the_os_and_writes_nothing() {
        let switch = FakeSwitch::new(false);
        assert_eq!(read(&switch), Ok(false));
        assert_eq!(*switch.calls.borrow(), vec!["is_on"]);
    }

    #[test]
    fn turning_it_on_reports_the_state_the_os_now_has() {
        let switch = FakeSwitch::new(false);
        assert_eq!(write(&switch, true), Ok(true));
        assert_eq!(*switch.calls.borrow(), vec!["turn_on", "is_on"]);
    }

    #[test]
    fn turning_it_off_reports_off() {
        let switch = FakeSwitch::new(true);
        assert_eq!(write(&switch, false), Ok(false));
        assert_eq!(*switch.calls.borrow(), vec!["turn_off", "is_on"]);
    }

    #[test]
    fn a_write_the_os_ignored_is_reported_as_not_on() {
        // The answer is the OS's state, not what was asked: the toggle must not show a change that
        // did not happen.
        let mut switch = FakeSwitch::new(false);
        switch.stuck = true;
        assert_eq!(write(&switch, true), Ok(false));
    }

    #[test]
    fn a_failed_write_is_an_error_and_nothing_is_reported_on() {
        let mut switch = FakeSwitch::new(false);
        switch.fail = true;
        assert_eq!(write(&switch, true), Err("registry denied".to_string()));
        assert_eq!(*switch.calls.borrow(), vec!["turn_on"], "no read-back after a failed write");
    }
}

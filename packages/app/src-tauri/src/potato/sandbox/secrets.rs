//! The secrets of one sandbox run. The app owns them: they are made here, pushed into the distro over
//! stdin as root (`RunSecrets::push_payload`, used by `lifecycle.rs`) and kept in memory only. The guest
//! never invents one when the app gave it (`pm-sbx-start-desktop.sh` uses the pushed files).
//!
//! Every type here has no `Clone`, no `Serialize`, no `Display`, and a `Debug` that prints no value; the
//! memory is overwritten when the value is dropped. `expose` is how a value is read; `guard_tests` checks
//! which files call it.

use rand::RngCore;

/// Where the guest finds the pushed files (a memory file system; empty after every stop).
pub const SECRETS_DIR: &str = "/run/pm-sbx";
pub const TOKEN_FILE: &str = "/run/pm-sbx/control.token";
pub const VIEW_FILE: &str = "/run/pm-sbx/vnc-view.pass";
pub const CONTROL_FILE: &str = "/run/pm-sbx/vnc-control.pass";

/// VNC authentication reads the first 8 characters of a password.
pub const VNC_PASSWORD_LEN: usize = 8;
const ALPHABET: &[u8; 62] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";

fn wipe(text: &mut String) {
    // Best effort: the memory holding the secret is overwritten before it is freed.
    let mut bytes = std::mem::take(text).into_bytes();
    for b in bytes.iter_mut() {
        *b = 0;
    }
    std::hint::black_box(&bytes);
}

/// The secret that opens the control service. 32 random bytes as hex.
pub struct ControlToken(String);

impl ControlToken {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut bytes);
        Self(bytes.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// The value, for the one request header and the one push into the distro.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for ControlToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ControlToken(..)")
    }
}

impl Drop for ControlToken {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// One VNC password: 8 random letters and digits.
pub struct VncPassword(String);

impl VncPassword {
    pub fn generate() -> Self {
        let mut out = String::with_capacity(VNC_PASSWORD_LEN);
        let mut byte = [0u8; 1];
        while out.len() < VNC_PASSWORD_LEN {
            rand::rngs::OsRng.fill_bytes(&mut byte);
            // 248 = 4 * 62: values above it are dropped so every character is equally likely.
            if byte[0] < 248 {
                out.push(ALPHABET[usize::from(byte[0]) % ALPHABET.len()] as char);
            }
        }
        Self(out)
    }

    /// The value, for the one push into the distro and (later) for the viewer link.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for VncPassword {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("VncPassword(..)")
    }
}

impl Drop for VncPassword {
    fn drop(&mut self) {
        wipe(&mut self.0);
    }
}

/// The two passwords of the screen: `view` shows it (the server drops every input event), `control` also
/// takes the mouse and keyboard (take-over). They always differ.
pub struct ScreenSecrets {
    view: VncPassword,
    control: VncPassword,
}

impl ScreenSecrets {
    pub fn generate() -> Self {
        let view = VncPassword::generate();
        let mut control = VncPassword::generate();
        while control.expose() == view.expose() {
            control = VncPassword::generate();
        }
        Self { view, control }
    }

    pub fn view(&self) -> &VncPassword {
        &self.view
    }

    pub fn control(&self) -> &VncPassword {
        &self.control
    }
}

impl std::fmt::Debug for ScreenSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScreenSecrets(..)")
    }
}

/// Everything made for one start.
pub struct RunSecrets {
    pub token: std::sync::Arc<ControlToken>,
    pub screen: std::sync::Arc<ScreenSecrets>,
}

impl RunSecrets {
    pub fn generate() -> Self {
        Self { token: std::sync::Arc::new(ControlToken::generate()), screen: std::sync::Arc::new(ScreenSecrets::generate()) }
    }

    /// The script (no secret in it) that writes the three files from stdin, and the stdin it reads: the
    /// token, the view password and the control password, one per line. Files are owned by `pm`, mode 0400.
    pub fn push(&self) -> (String, Vec<u8>) {
        let script = format!(
            "umask 077; mkdir -p {SECRETS_DIR} && chmod 0755 {SECRETS_DIR} && IFS= read -r t && IFS= read -r v && IFS= read -r c && \
             printf '%s' \"$t\" > {TOKEN_FILE} && printf '%s' \"$v\" > {VIEW_FILE} && printf '%s' \"$c\" > {CONTROL_FILE} && \
             chown pm:pm {TOKEN_FILE} {VIEW_FILE} {CONTROL_FILE} && chmod 0400 {TOKEN_FILE} {VIEW_FILE} {CONTROL_FILE}"
        );
        let stdin = format!("{}\n{}\n{}\n", self.token.expose(), self.screen.view().expose(), self.screen.control().expose());
        (script, stdin.into_bytes())
    }
}

impl std::fmt::Debug for RunSecrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunSecrets(..)")
    }
}

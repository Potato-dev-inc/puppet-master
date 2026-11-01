//! The sandbox screen links: how the viewer page and its websocket are allowed to reach the sandbox desktop.
//!
//! A **sealed link** is `<payload>.<tag>`: the payload is the claims as compact JSON (base64url), the tag is
//! HMAC-SHA256 over the payload text with a key made at app start and kept in memory only (no file, no log, no
//! `Debug`). The claims are: bot id, sandbox name, generation, mode (`view` or `control`), lease fence, expiry,
//! and who the link is for (this computer's app, or one paired phone). Changing any of them breaks the tag.
//!
//! What a link is good for:
//!
//! - **Time**: one hour (`LINK_TTL_MS`).
//! - **Generation**: every start and stop of the sandbox bumps it (`ScreenTarget::generation`); a link made for
//!   another generation is dead, so a restart kills every open viewer.
//! - **Mode**: a `control` link is made only while the PERSON holds the lease, and it is valid only while that very
//!   lease (same fence) is the live holder. An open control connection is checked every second
//!   (`RECHECK_EVERY`) and closed the moment the lease is lost (handed back, expired, taken again, generation bump).
//!   A `view` link never depends on the lease: you can watch the bot work and watch yourself take over.
//! - **Who**: a link made for the app is refused when a phone presents it and the other way round.
//!
//! The VNC password is NOT in the link. `ScreenHub::link` puts it in the URL fragment of the iframe address it
//! returns (`#vnc=...`): a fragment never reaches a server or a log. The link itself (the path) is not enough to
//! see the screen: the websocket still needs the password at the VNC level.
//!
//! The pure parts (`ScreenKey`, `Claims`, `ScreenHub`) take the clock and the sandbox as arguments, so the tests use
//! neither a real clock nor a real sandbox. `bridge/routes/potato_screen.rs` serves the viewer files and relays the
//! websocket; `screen_live.rs` is the app's one hub and its Tauri events.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use parking_lot::Mutex;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use super::lease::{parse_lease_id, Banner, Displaced, Holder, LeaseError, ScreenLease};
use super::secrets::ScreenSecrets;
use super::{Sandbox, SbxResult, ScreenTarget};

type HmacSha256 = Hmac<Sha256>;

/// A sealed link lasts one hour.
pub const LINK_TTL_MS: i64 = 60 * 60 * 1000;
/// An open control connection is checked against the lease this often.
pub const RECHECK_EVERY: std::time::Duration = std::time::Duration::from_secs(1);
/// The route prefix (after the bridge prefix): `/potato/sandbox/screen/<sealed>/<file>`.
pub const ROUTE_ROOT: [&str; 3] = ["potato", "sandbox", "screen"];
/// The wire code a phone gets when it asks for control without the person's yes at the PC.
pub const PHONE_NEEDS_DESKTOP_YES: &str = "PHONE_NEEDS_DESKTOP_YES";
const CLAIMS_VERSION: u8 = 1;
const MAX_BOT_CHARS: usize = 64;

/// The path segments `["potato","sandbox","screen", <sealed>, rest...]` -> `(sealed, rest)`.
pub fn route_parts<'a>(segments: &'a [&'a str]) -> Option<(&'a str, &'a [&'a str])> {
    let segments = crate::mobile_pairing::normalize_bridge_segments(segments);
    if segments.len() >= 4 && segments[..3] == ROUTE_ROOT {
        Some((segments[3], &segments[4..]))
    } else {
        None
    }
}

// ---- the key and the claims ----------------------------------------------------------------------

/// The HMAC key. Random per app start, in memory only: no `Clone`, no `Debug` value, wiped on drop.
pub struct ScreenKey([u8; 32]);

impl ScreenKey {
    pub fn generate() -> Self {
        let mut key = [0u8; 32];
        rand::rngs::OsRng.fill_bytes(&mut key);
        Self(key)
    }

    fn mac(&self) -> HmacSha256 {
        // (HMAC takes any key length.)
        HmacSha256::new_from_slice(&self.0).expect("hmac key")
    }

    /// Seals `claims`: `<payload>.<tag>`.
    pub fn seal(&self, claims: &Claims) -> String {
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap_or_default());
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        format!("{payload}.{}", URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes()))
    }

    /// Opens `sealed`: the tag must match (compared in constant time). Says nothing about time, generation or lease.
    pub fn open(&self, sealed: &str) -> Result<Claims, ScreenError> {
        let (payload, tag) = sealed.split_once('.').ok_or(ScreenError::LinkBad)?;
        if sealed.len() > 1024 || tag.contains('.') {
            return Err(ScreenError::LinkBad);
        }
        let tag = URL_SAFE_NO_PAD.decode(tag).map_err(|_| ScreenError::LinkBad)?;
        let mut mac = self.mac();
        mac.update(payload.as_bytes());
        mac.verify_slice(&tag).map_err(|_| ScreenError::LinkBad)?;
        let bytes = URL_SAFE_NO_PAD.decode(payload).map_err(|_| ScreenError::LinkBad)?;
        let claims: Claims = serde_json::from_slice(&bytes).map_err(|_| ScreenError::LinkBad)?;
        if claims.v != CLAIMS_VERSION {
            return Err(ScreenError::LinkBad);
        }
        Ok(claims)
    }
}

impl std::fmt::Debug for ScreenKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScreenKey(..)")
    }
}

impl Drop for ScreenKey {
    fn drop(&mut self) {
        for b in self.0.iter_mut() {
            *b = 0;
        }
        std::hint::black_box(&self.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    View,
    Control,
}

impl Mode {
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "view" => Some(Mode::View),
            "control" => Some(Mode::Control),
            _ => None,
        }
    }
}

/// Who a link is for. A phone is named by its pairing id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Audience {
    App,
    Device(String),
}

impl Audience {
    fn wire(&self) -> String {
        match self {
            Audience::App => "app".to_string(),
            Audience::Device(id) => format!("dev:{id}"),
        }
    }

    fn from_wire(text: &str) -> Option<Self> {
        match text {
            "app" => Some(Audience::App),
            other => other.strip_prefix("dev:").filter(|id| !id.is_empty()).map(|id| Audience::Device(id.to_string())),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub v: u8,
    /// The bot the panel belongs to.
    pub b: String,
    /// The sandbox name.
    pub s: String,
    /// The sandbox generation.
    pub g: u64,
    pub m: Mode,
    /// The lease fence: for `control`, the person's lease; for `view`, the highest fence at the time (information only).
    pub f: u64,
    /// Expiry, epoch milliseconds.
    pub x: i64,
    /// The audience, as `app` or `dev:<id>`.
    pub a: String,
}

impl Claims {
    pub fn audience(&self) -> Option<Audience> {
        Audience::from_wire(&self.a)
    }
}

// ---- errors --------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenError {
    /// The Labs switches are off.
    Off,
    /// Not a link we made (or changed).
    LinkBad,
    Expired,
    /// Made for another start of the sandbox.
    Stale,
    /// For the other kind of caller (app or phone).
    WrongAudience,
    /// A control link whose lease is not the live holder's any more.
    LeaseLost,
    /// A control link was asked for and the person does not hold the screen.
    NeedTakeOver,
    /// The sandbox is not running (no passwords to give).
    NotRunning,
    /// A phone asked for control and the person has not said yes at the PC.
    PhoneNeedsDesktopYes,
    BadBot,
    /// The sandbox itself said no (`code`, plain sentence).
    Sandbox(&'static str, String),
}

impl ScreenError {
    pub fn code(&self) -> &'static str {
        match self {
            ScreenError::Off => "SCREEN_OFF",
            ScreenError::LinkBad => "SCREEN_LINK_BAD",
            ScreenError::Expired => "SCREEN_LINK_EXPIRED",
            ScreenError::Stale => "SCREEN_LINK_STALE",
            ScreenError::WrongAudience => "SCREEN_WRONG_AUDIENCE",
            ScreenError::LeaseLost => "SCREEN_LEASE_LOST",
            ScreenError::NeedTakeOver => "SCREEN_NEED_TAKE_OVER",
            ScreenError::NotRunning => "SCREEN_NOT_RUNNING",
            ScreenError::PhoneNeedsDesktopYes => PHONE_NEEDS_DESKTOP_YES,
            ScreenError::BadBot => "SCREEN_BAD_BOT",
            ScreenError::Sandbox(code, _) => code,
        }
    }

    pub fn message(&self) -> String {
        match self {
            ScreenError::Off => "The Computer is turned off.".to_string(),
            ScreenError::LinkBad => "This screen link is not good.".to_string(),
            ScreenError::Expired => "This screen link ran out. Open the Computer again.".to_string(),
            ScreenError::Stale => "The sandbox was restarted. Open the Computer again.".to_string(),
            ScreenError::WrongAudience => "This screen link is for another device.".to_string(),
            ScreenError::LeaseLost => "You no longer have control.".to_string(),
            ScreenError::NeedTakeOver => "Take over first, then open control.".to_string(),
            ScreenError::NotRunning => "The sandbox is not running yet.".to_string(),
            ScreenError::PhoneNeedsDesktopYes => "Say yes on your PC first, then this phone can take control.".to_string(),
            ScreenError::BadBot => "That bot name is not good.".to_string(),
            ScreenError::Sandbox(_, message) => message.clone(),
        }
    }

    /// The HTTP status the bridge answers with.
    pub fn status(&self) -> u16 {
        match self {
            ScreenError::Off | ScreenError::LinkBad => 404,
            ScreenError::Expired | ScreenError::Stale => 410,
            ScreenError::WrongAudience | ScreenError::LeaseLost | ScreenError::PhoneNeedsDesktopYes => 403,
            ScreenError::NeedTakeOver | ScreenError::NotRunning => 409,
            ScreenError::BadBot => 400,
            ScreenError::Sandbox(..) => 503,
        }
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({ "code": self.code(), "error": self.message() })
    }
}

impl std::fmt::Display for ScreenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for ScreenError {}

impl From<LeaseError> for ScreenError {
    fn from(_: LeaseError) -> Self {
        ScreenError::LeaseLost
    }
}

// ---- where the screen comes from -----------------------------------------------------------------

/// What the hub needs of a sandbox: its name and where its screen is. Every `Sandbox` is one.
pub trait ScreenSource: Send + Sync {
    fn name(&self) -> String;
    fn target(&self) -> SbxResult<ScreenTarget>;
    /// The person holds the screen until `until_ms` (see `Sandbox::person_holds`).
    fn person_holds(&self, _until_ms: i64) {}
}

impl<T: Sandbox + ?Sized> ScreenSource for T {
    fn name(&self) -> String {
        self.status().map(|s| s.name).unwrap_or_default()
    }

    fn target(&self) -> SbxResult<ScreenTarget> {
        Sandbox::screen_target(self)
    }

    fn person_holds(&self, until_ms: i64) {
        Sandbox::person_holds(self, until_ms)
    }
}

// ---- what the hub says ---------------------------------------------------------------------------

/// An iframe address with the VNC password in its fragment. It prints nothing and is read once with `into_inner`.
pub struct ScreenSrc(String);

impl ScreenSrc {
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for ScreenSrc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ScreenSrc(..)")
    }
}

/// What a link is allowed to reach right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The guest's websocket forward on this computer.
    pub port: u16,
    pub claims: Claims,
}

/// The header words and buttons of the Computer panel.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenState {
    pub enabled: bool,
    /// `free`, `bot` ("<bot> is using it"), `you` ("You have control") or `needs_you` ("Needs you").
    pub phase: &'static str,
    pub bot: Option<String>,
    pub card_id: Option<String>,
    pub reason: Option<String>,
    /// When the "Needs you" card runs out.
    pub card_expires_ms: Option<i64>,
}

/// Things the person's buttons did (the brain side learns of them through these; the page re-reads the state).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenEvent {
    /// The person took the screen. `displaced` is the bot run that had it.
    TakenOver { displaced: Option<Displaced> },
    /// The person gave it back ("I'm done"). `displaced` is the run that can take a new lease.
    HandedBack { displaced: Option<Displaced> },
    /// The person turned a "Needs you" card down.
    Skipped { bot: String, run_id: String },
    /// The clock ended a lease or a card, or the Labs switch changed.
    Changed,
}

type Listener = Arc<dyn Fn(&ScreenEvent) + Send + Sync>;

pub struct ScreenHub {
    key: ScreenKey,
    source: Mutex<Option<Arc<dyn ScreenSource>>>,
    /// The sandbox's name, asked once (a real sandbox answers by asking the system).
    name: Mutex<Option<String>>,
    lease: Arc<Mutex<ScreenLease>>,
    phone_yes: Mutex<HashSet<String>>,
    enabled: AtomicBool,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    listener: Mutex<Option<Listener>>,
    last_state: Mutex<Option<ScreenState>>,
}

fn real_now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn valid_bot(bot: &str) -> bool {
    !bot.is_empty() && bot.chars().count() <= MAX_BOT_CHARS && bot.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl ScreenHub {
    pub fn new() -> Self {
        Self::with_clock(Box::new(real_now_ms))
    }

    pub fn with_clock(clock: Box<dyn Fn() -> i64 + Send + Sync>) -> Self {
        Self {
            key: ScreenKey::generate(),
            source: Mutex::new(None),
            name: Mutex::new(None),
            lease: Arc::new(Mutex::new(ScreenLease::new())),
            phone_yes: Mutex::new(HashSet::new()),
            enabled: AtomicBool::new(false),
            clock,
            listener: Mutex::new(None),
            last_state: Mutex::new(None),
        }
    }

    pub fn now_ms(&self) -> i64 {
        (self.clock)()
    }

    /// The sandbox the screen belongs to (set when the app has one).
    pub fn set_source(&self, source: Option<Arc<dyn ScreenSource>>) {
        *self.source.lock() = source;
        *self.name.lock() = None;
    }

    fn name_of(&self, source: &Arc<dyn ScreenSource>) -> String {
        let mut cached = self.name.lock();
        if cached.is_none() {
            let name = source.name();
            if !name.is_empty() {
                *cached = Some(name);
            }
        }
        cached.clone().unwrap_or_default()
    }

    pub fn set_listener(&self, listener: Option<Listener>) {
        *self.listener.lock() = listener;
    }

    /// Both Labs switches (`potato_bot_enabled` and `potato_sandbox_enabled`) are on. Off: no link works.
    pub fn set_enabled(&self, on: bool) {
        let was = self.enabled.swap(on, Ordering::SeqCst);
        if was != on {
            self.emit(&ScreenEvent::Changed);
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// The lease table. The brain's tool loop takes the bot's lease from here (one screen, one lease).
    pub fn lease(&self) -> &Arc<Mutex<ScreenLease>> {
        &self.lease
    }

    /// The person said yes, at this PC, to a phone taking control. (The confirm card that calls this is not built
    /// yet: until it is, a phone asking for control gets `PHONE_NEEDS_DESKTOP_YES`.)
    pub fn allow_phone_control(&self, device_id: &str) {
        self.phone_yes.lock().insert(device_id.to_string());
    }

    pub fn revoke_phone_control(&self, device_id: &str) {
        self.phone_yes.lock().remove(device_id);
    }

    fn emit(&self, event: &ScreenEvent) {
        let listener = self.listener.lock().clone();
        if let Some(listener) = listener {
            listener(event);
        }
    }

    fn source(&self) -> Result<Arc<dyn ScreenSource>, ScreenError> {
        self.source
            .lock()
            .clone()
            .ok_or_else(|| ScreenError::Sandbox("SANDBOX_NOT_INSTALLED", "The sandbox is not set up yet.".to_string()))
    }

    fn target(&self) -> Result<(Arc<dyn ScreenSource>, ScreenTarget), ScreenError> {
        let source = self.source()?;
        let target = source.target().map_err(|e| ScreenError::Sandbox(e.code, e.message))?;
        Ok((source, target))
    }

    // ---- the state and the buttons ----

    pub fn state(&self) -> ScreenState {
        let now = self.now_ms();
        let lease = self.lease.lock();
        let (phase, bot, card_id) = match lease.banner(now) {
            Banner::Free => ("free", None, None),
            Banner::BotUsing { bot } => ("bot", Some(bot), None),
            Banner::PersonHasControl => ("you", None, None),
            Banner::NeedsYou { bot, card_id } => ("needs_you", Some(bot), Some(card_id)),
        };
        let card = lease.card(now);
        ScreenState {
            enabled: self.enabled(),
            phase,
            bot,
            card_id,
            reason: card.map(|c| c.reason.clone()),
            card_expires_ms: card.map(|c| c.expires_ms),
        }
    }

    /// "Take over": the person takes the screen with a new, higher fence.
    pub fn take_over(&self) -> Result<ScreenState, ScreenError> {
        self.require_enabled()?;
        let (_, displaced) = self.lease.lock().take_over(self.now_ms());
        self.emit(&ScreenEvent::TakenOver { displaced });
        Ok(self.state())
    }

    /// "Hand back": the person is done. Every control link dies with the lease.
    pub fn hand_back(&self) -> Result<ScreenState, ScreenError> {
        self.require_enabled()?;
        let displaced = self.lease.lock().hand_back(self.now_ms());
        self.emit(&ScreenEvent::HandedBack { displaced });
        Ok(self.state())
    }

    /// "Skip": turns a "Needs you" card down. The bot keeps the screen and moves on.
    pub fn skip(&self, card_id: &str) -> Result<ScreenState, ScreenError> {
        self.require_enabled()?;
        let card = self.lease.lock().skip(card_id)?;
        self.emit(&ScreenEvent::Skipped { bot: card.bot, run_id: card.run_id });
        Ok(self.state())
    }

    /// The person is still working in the control view: the lease lasts 15 more minutes. Fails when it is not the
    /// person's live lease (then there is nothing to keep).
    pub fn keep(&self) -> Result<(), ScreenError> {
        self.require_enabled()?;
        let now = self.now_ms();
        let mut lease = self.lease.lock();
        match lease.current(now) {
            Some(l) if l.holder == Holder::Person => {
                let id = l.id.clone();
                lease.renew(&id, now)?;
                let until = lease.current(now).map_or(now, |l| l.expires_ms);
                drop(lease);
                if let Ok(source) = self.source() {
                    source.person_holds(until);
                }
                Ok(())
            }
            _ => Err(ScreenError::LeaseLost),
        }
    }

    /// Clears what the clock ended and tells the listener when the state changed (the keeper thread calls it).
    pub fn tick(&self) {
        let now = self.now_ms();
        self.lease.lock().tick(now);
        let state = self.state();
        let mut last = self.last_state.lock();
        if last.as_ref() != Some(&state) {
            let first = last.is_none();
            *last = Some(state);
            drop(last);
            if !first {
                self.emit(&ScreenEvent::Changed);
            }
        }
    }

    fn require_enabled(&self) -> Result<(), ScreenError> {
        if self.enabled() {
            Ok(())
        } else {
            Err(ScreenError::Off)
        }
    }

    // ---- making links ----

    /// The iframe address for `bot`, with the password in the fragment. `base` is the address the page reaches the
    /// bridge at (`http://127.0.0.1:<port>` for the app, empty for a phone: its own origin) and ends without `/`.
    ///
    /// `control` needs the person to hold the screen (`take_over` first) and, for a phone, the person's yes at the PC.
    pub fn link(&self, bot: &str, mode: Mode, audience: &Audience, base: &str) -> Result<ScreenSrc, ScreenError> {
        self.require_enabled()?;
        if !valid_bot(bot) {
            return Err(ScreenError::BadBot);
        }
        if mode == Mode::Control {
            if let Audience::Device(id) = audience {
                if !self.phone_yes.lock().contains(id) {
                    return Err(ScreenError::PhoneNeedsDesktopYes);
                }
            }
        }
        let (source, target) = self.target()?;
        let secrets = target.passwords.clone().ok_or(ScreenError::NotRunning)?;
        let now = self.now_ms();
        // A link is a screen the person is about to use: the sandbox stays up for its life (an hour at most).
        source.person_holds(now + LINK_TTL_MS);
        let fence = {
            let lease = self.lease.lock();
            match (mode, lease.current(now)) {
                (Mode::Control, Some(l)) if l.holder == Holder::Person => parse_lease_id(&l.id).fence,
                (Mode::Control, _) => return Err(ScreenError::NeedTakeOver),
                (Mode::View, _) => lease.highest_fence(),
            }
        };
        let claims = Claims {
            v: CLAIMS_VERSION,
            b: bot.to_string(),
            s: self.name_of(&source),
            g: target.generation,
            m: mode,
            f: fence,
            x: now + LINK_TTL_MS,
            a: audience.wire(),
        };
        let sealed = self.key.seal(&claims);
        let query = if mode == Mode::Control { "?control=1" } else { "" };
        let password = fragment_password(&secrets, mode);
        Ok(ScreenSrc(format!("{base}/bridge/potato/sandbox/screen/{sealed}/embed.html{query}#vnc={password}")))
    }

    // ---- checking links ----

    /// Who a sealed link is for, when its tag is good and it has not run out. Used by the bridge gate to pick the
    /// caller kind before the request is served; says nothing about generation or lease.
    pub fn peek_audience(&self, sealed: &str) -> Option<Audience> {
        let claims = self.key.open(sealed).ok()?;
        (claims.x > self.now_ms()).then(|| claims.audience()).flatten()
    }

    /// Everything a request with this link must pass: the tag, the clock, the audience, the sandbox, the generation
    /// and (for control) the lease.
    pub fn open(&self, sealed: &str, audience: &Audience) -> Result<Grant, ScreenError> {
        self.require_enabled()?;
        let claims = self.key.open(sealed)?;
        self.check(&claims, audience)
    }

    /// The same checks again for a connection that is already open.
    pub fn revalidate(&self, grant: &Grant, audience: &Audience) -> Result<(), ScreenError> {
        self.require_enabled()?;
        self.check(&grant.claims, audience).map(|_| ())
    }

    fn check(&self, claims: &Claims, audience: &Audience) -> Result<Grant, ScreenError> {
        let now = self.now_ms();
        if claims.x <= now {
            return Err(ScreenError::Expired);
        }
        if claims.audience().as_ref() != Some(audience) {
            return Err(ScreenError::WrongAudience);
        }
        let (source, target) = self.target()?;
        if claims.s != self.name_of(&source) || claims.g != target.generation {
            return Err(ScreenError::Stale);
        }
        if claims.m == Mode::Control {
            let lease = self.lease.lock();
            match lease.current(now) {
                Some(l) if l.holder == Holder::Person && parse_lease_id(&l.id).fence == claims.f => {}
                _ => return Err(ScreenError::LeaseLost),
            }
            if let Audience::Device(id) = audience {
                if !self.phone_yes.lock().contains(id) {
                    return Err(ScreenError::PhoneNeedsDesktopYes);
                }
            }
        }
        Ok(Grant { port: target.view_port, claims: claims.clone() })
    }
}

impl Default for ScreenHub {
    fn default() -> Self {
        Self::new()
    }
}

/// The one place a VNC password is read for a link.
fn fragment_password(secrets: &ScreenSecrets, mode: Mode) -> String {
    let password = match mode {
        Mode::View => secrets.view(),
        Mode::Control => secrets.control(),
    };
    password.expose().to_string()
}

// ---- the app's one hub ---------------------------------------------------------------------------

static HUB: OnceLock<Arc<ScreenHub>> = OnceLock::new();

/// The process-wide hub (made on first use).
pub fn hub() -> &'static Arc<ScreenHub> {
    HUB.get_or_init(|| Arc::new(ScreenHub::new()))
}

// ---- the viewer files ----------------------------------------------------------------------------
//
// Pure helpers for `bridge/routes/potato_screen.rs`, which reads the files and answers the requests. They live
// here because the viewer page names the sandbox in its own words.

/// The websocket path under a sealed link (the viewer's default `path`).
pub const WEBSOCKET_FILE: &str = "websockify";
const PARENT_META_EMPTY: &str = r#"name="pm-sbx-parent-origins" content="""#;
const MAX_FILE_PATH_CHARS: usize = 120;
/// The webview's own host on Windows (the Tauri default scheme is http, its twin is https).
const APP_HOST: &str = "tauri.localhost";

/// The file under the viewer folder a request names, as a relative `/` path, or `None` when it is not one of the
/// viewer's own files: only letters, digits, `_`, `-`, `.` and `/` in short segments, never `..`, no empty segment,
/// no leading dot. An empty list is the page itself.
pub fn viewer_file(rest: &[&str]) -> Option<String> {
    if rest.is_empty() {
        return Some("embed.html".to_string());
    }
    let ok_segment = |s: &str| {
        !s.is_empty()
            && s.len() <= 60
            && !s.starts_with('.')
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    };
    if !rest.iter().all(|s| ok_segment(s)) {
        return None;
    }
    let path = rest.join("/");
    if path.len() > MAX_FILE_PATH_CHARS {
        return None;
    }
    let top = rest[0];
    let allowed = rest.len() == 1 && matches!(top, "embed.html" | "clipboard-bridge.js" | "mobile-keyboard.js") || (rest.len() > 1 && top == "novnc");
    allowed.then_some(path)
}

pub fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "wasm" => "application/wasm",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.trim().split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    (!authority.is_empty() && !authority.contains('@')).then(|| format!("{scheme}://{authority}"))
}

/// The origins the viewer page may post its status to (and that may frame it): the webview's own, the dev server
/// the request came from when that is on this computer, and, for a phone, the origin of the page that framed it.
///
/// A phone's request has passed the tunnel proxy, which rewrites `Host`, so the phone page's origin comes from
/// `X-Forwarded-Host` (with `X-Forwarded-Proto`), from `Host` (a websocket upgrade keeps it) or from `Referer`.
/// This only widens what the answer to THAT request allows, and the request already holds a link made for one
/// paired phone; the answer holds no secret (the password is in the fragment the page reads itself).
pub fn parent_origins(headers: &str, remote: bool) -> Vec<String> {
    use crate::mobile_pairing::{header_value, is_app_origin};
    let mut origins: Vec<String> = ["http", "https"].iter().map(|scheme| format!("{scheme}://{APP_HOST}")).collect();
    let referer = header_value(headers, "Referer").and_then(origin_of);
    if let Some(origin) = referer.clone().filter(|o| is_app_origin(o)) {
        origins.push(origin);
    }
    if remote {
        origins.extend(referer);
        let proto = header_value(headers, "X-Forwarded-Proto").and_then(|p| p.split(',').next()).map(str::trim).unwrap_or("https");
        let proto = if proto == "http" { "http" } else { "https" };
        for name in ["X-Forwarded-Host", "Host"] {
            let host = header_value(headers, name).and_then(|h| h.split(',').next()).map(str::trim).unwrap_or("");
            let origin = format!("{proto}://{host}");
            // (A loopback `Host` is the proxy talking to the bridge: not the phone's page.)
            if !host.is_empty() && host.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']')) && !is_app_origin(&origin) {
                origins.push(origin);
            }
        }
    }
    origins.sort();
    origins.dedup();
    origins
}

/// The viewer page with the parent origins filled into its allowlist tag. A page without the (empty) tag is
/// returned unchanged, which leaves it posting nothing.
pub fn fill_viewer_page(html: &str, origins: &[String]) -> String {
    let filled = format!(r#"name="pm-sbx-parent-origins" content="{}""#, origins.join(","));
    html.replacen(PARENT_META_EMPTY, &filled, 1)
}

/// The `Content-Security-Policy` that lets only these origins (and the page's own) frame the viewer.
pub fn frame_policy(origins: &[String]) -> String {
    format!("frame-ancestors 'self' {}", origins.join(" "))
}

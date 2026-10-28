//! Text Your Agents: the bot token, kept only in the OS credential store (decision D-1).
//!
//! There is no file fallback. If the store cannot be used, saving refuses. The token is masked by
//! `redact::known` as soon as it is saved or read, and nothing here ever puts it in an error message.
//! The UI never gets the token back, only the bot's `@name` from Telegram.

use crate::redact::known;
use crate::secrets::{self, SecretBackend};
use once_cell::sync::Lazy;
use regex::Regex;

/// Shown when the credential store cannot be used.
pub const NO_STORE: &str = "This computer has no safe place for the token, so Telegram can't be used.";
/// Shown when the text is not shaped like a BotFather token.
pub const BAD_FORMAT: &str = "That does not look like a bot token. Copy the whole token from BotFather.";
/// Shown when the store did not keep the token (write failed or read back differently).
pub const NOT_KEPT: &str = "The token could not be saved in the credential store.";
/// Shown when the store could not be read.
pub const NOT_READ: &str = "The token could not be read from the credential store.";

static SHAPE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[0-9]{5,12}:[A-Za-z0-9_-]{30,60}$").expect("token shape"));

/// `<digits>:<secret>` as BotFather prints it. Exact: no trimming here (saving trims first).
pub fn looks_like_token(raw: &str) -> bool {
    SHAPE.is_match(raw)
}

/// The credential store name for a profile: `channels/<profile or default>/telegram_bot_token`.
pub fn key_name_for(profile: Option<&str>) -> String {
    format!("channels/{}/telegram_bot_token", profile.unwrap_or("default"))
}

/// Store the token. Refuses when the store is unusable, the shape is wrong, or the read-back differs.
pub fn save_with(backend: &dyn SecretBackend, profile: Option<&str>, raw: &str) -> Result<(), String> {
    if !backend.available() {
        return Err(NO_STORE.into());
    }
    let token = raw.trim();
    if !looks_like_token(token) {
        return Err(BAD_FORMAT.into());
    }
    // Mask first, so nothing after this point can show the token.
    known::register(token);
    let name = key_name_for(profile);
    if backend.set(&name, token).is_err() {
        return Err(NOT_KEPT.into());
    }
    match backend.get(&name) {
        Ok(Some(back)) if back == token => Ok(()),
        _ => Err(NOT_KEPT.into()),
    }
}

/// The saved token, or `None` when there is none. Masked from this point on.
pub fn read_with(backend: &dyn SecretBackend, profile: Option<&str>) -> Result<Option<String>, String> {
    let token = backend.get(&key_name_for(profile)).map_err(|_| NOT_READ.to_string())?;
    let token = token.filter(|v| !v.is_empty());
    if let Some(value) = &token {
        known::register(value);
    }
    Ok(token)
}

/// Delete the saved token. Deleting when there is none is fine.
pub fn forget_with(backend: &dyn SecretBackend, profile: Option<&str>) -> Result<(), String> {
    backend.delete(&key_name_for(profile)).map_err(|_| NOT_KEPT.to_string())
}

/// The process's real store, for this profile.
pub fn save(raw: &str) -> Result<(), String> {
    save_with(secrets::backend(), crate::app_paths::profile().as_deref(), raw)
}

pub fn read() -> Result<Option<String>, String> {
    read_with(secrets::backend(), crate::app_paths::profile().as_deref())
}

pub fn forget() -> Result<(), String> {
    forget_with(secrets::backend(), crate::app_paths::profile().as_deref())
}

/// Whether the credential store can be used at all, and its plain-language name for the UI.
pub fn store_available() -> bool {
    secrets::backend().available()
}

pub fn store_label() -> &'static str {
    secrets::backend().label()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::MemoryBackend;

    /// A fake fixture token, never a real one. Long enough to be masked (`known::register` needs 6+).
    const TOKEN: &str = "123456789:TESTtokenTESTtokenTESTtokenTESTtok";

    /// Accepts writes and forgets them: the read-back check must catch it.
    struct ForgetfulBackend;

    impl SecretBackend for ForgetfulBackend {
        fn get(&self, _name: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn set(&self, _name: &str, _value: &str) -> Result<(), String> {
            Ok(())
        }
        fn delete(&self, _name: &str) -> Result<(), String> {
            Ok(())
        }
        fn label(&self) -> &'static str {
            "forgetful"
        }
        fn available(&self) -> bool {
            true
        }
    }

    #[test]
    fn token_shape_table() {
        let valid = [
            TOKEN,
            "12345:TESTtokenTESTtokenTESTtokenTESTtok",            // 5-digit id, the minimum
            "123456789012:TESTtokenTESTtokenTESTtokenTESTtok",     // 12-digit id, the maximum
            "123456789:abcdefghijklmnopqrstuvwxyz0123",            // secret of 30 characters
            "123456789:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWX", // 60 characters
            "123456789:under_score-and-dash_0123456789abcdef",     // _ and - are allowed
        ];
        for token in valid {
            assert!(looks_like_token(token), "should accept {token}");
        }
        let invalid = [
            "",
            "1234:TESTtokenTESTtokenTESTtokenTESTtok",             // 4-digit id
            "1234567890123:TESTtokenTESTtokenTESTtokenTESTtok",    // 13-digit id
            "123456789:abcdefghijklmnopqrstuvwxyz012",             // secret of 29 characters
            "123456789:abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXY", // 61 characters
            "123456789:TESTtokenTESTtokenTESTtoken.TESTtok",       // a dot is not allowed
            "123456789 TESTtokenTESTtokenTESTtokenTESTtok",        // no colon
            " 123456789:TESTtokenTESTtokenTESTtokenTESTtok",       // leading space (exact check)
            "123456789:TESTtokenTESTtokenTESTtokenTESTtok\n",      // trailing newline (exact check)
            "\u{0661}23456789:TESTtokenTESTtokenTESTtokenTESTtok", // Arabic-Indic digit is not 0-9
            "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123",         // a provider key is not a bot token
        ];
        for token in invalid {
            assert!(!looks_like_token(token), "should refuse {token:?}");
        }
    }

    #[test]
    fn key_name_is_per_profile() {
        assert_eq!(key_name_for(None), "channels/default/telegram_bot_token");
        assert_eq!(key_name_for(Some("n32tg")), "channels/n32tg/telegram_bot_token");
    }

    #[test]
    fn save_refuses_when_the_store_is_unavailable() {
        let backend = MemoryBackend::unavailable();
        assert_eq!(save_with(&backend, None, TOKEN).unwrap_err(), NO_STORE);
        assert_eq!(read_with(&backend, None).unwrap_err(), NOT_READ);
    }

    #[test]
    fn save_refuses_a_bad_shape_and_stores_nothing() {
        let backend = MemoryBackend::new();
        let err = save_with(&backend, None, "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123").unwrap_err();
        assert_eq!(err, BAD_FORMAT);
        assert!(!err.contains("sk-ant"));
        assert_eq!(read_with(&backend, None).unwrap(), None);
    }

    #[test]
    fn save_trims_then_reads_back_the_same_token() {
        let backend = MemoryBackend::new();
        save_with(&backend, None, &format!("  {TOKEN}\n")).unwrap();
        assert_eq!(read_with(&backend, None).unwrap().as_deref(), Some(TOKEN));
        assert_eq!(backend.get("channels/default/telegram_bot_token").unwrap().as_deref(), Some(TOKEN));
    }

    #[test]
    fn name_follows_the_profile() {
        let backend = MemoryBackend::new();
        save_with(&backend, Some("n32tg"), TOKEN).unwrap();
        assert_eq!(backend.get("channels/n32tg/telegram_bot_token").unwrap().as_deref(), Some(TOKEN));
        assert_eq!(backend.get("channels/default/telegram_bot_token").unwrap(), None);
        assert_eq!(read_with(&backend, None).unwrap(), None);
    }

    #[test]
    fn save_refuses_when_the_store_does_not_keep_the_token() {
        let err = save_with(&ForgetfulBackend, None, TOKEN).unwrap_err();
        assert_eq!(err, NOT_KEPT);
        assert!(!err.contains(TOKEN));
    }

    #[test]
    fn forget_deletes_the_token() {
        let backend = MemoryBackend::new();
        save_with(&backend, None, TOKEN).unwrap();
        forget_with(&backend, None).unwrap();
        assert_eq!(read_with(&backend, None).unwrap(), None);
        forget_with(&backend, None).unwrap();
    }

    #[test]
    fn saved_token_is_masked_by_redact() {
        let backend = MemoryBackend::new();
        save_with(&backend, None, TOKEN).unwrap();
        let raw = format!("the bot said {TOKEN} to me");
        let masked = crate::redact::text(&raw);
        assert!(!masked.as_str().contains(TOKEN), "token still visible: {}", masked.as_str());
    }

    #[test]
    fn read_back_token_is_masked_by_redact() {
        let backend = MemoryBackend::new();
        backend.set("channels/default/telegram_bot_token", TOKEN).unwrap();
        assert_eq!(read_with(&backend, None).unwrap().as_deref(), Some(TOKEN));
        let masked = crate::redact::text(&format!("log line {TOKEN}"));
        assert!(!masked.as_str().contains(TOKEN));
    }
}

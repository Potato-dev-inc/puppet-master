//! Text Your Agents (Telegram first): chat channels that let the person text their agents from a phone.
//!
//! This is remote control of the person's PC through a third party, so every rule is conservative.
//! Layout (see `docs/features/telegram.md` once written):
//! - `config`: the saved settings file (`telegram.json`), its defaults and the phone view.
//! - `token`: the bot token, kept only in the OS credential store.
//! - `policy`: which attention cards may be answered from chat, and which capability each command needs.
//! - `render`: the single cleaning step for everything sent out (redaction, length cap, plain text).
//! - `grants`: single-use, short-lived tokens behind the bot's buttons.
//! - `rate`: message buckets (inbound, answers, outbound, strangers).
//! - `refs`: the numbered pane list, message references and conversation windows.
//! - `commands`: parses the text the person sends.
//! - `pairing`: the one-time code and the desktop confirm step.
//! - `host`: the trait the engine uses to reach the rest of the app (fake in tests).
//! - `engine`: handles one batch of updates from the bot (the inbound rules).
//! - `notify`: decides what goes out to the chat (needs-you cards, replies, run ends).
//! - `host_live`: the real `Host`, wired to the app's registry, attention and outbox.
//! - `telegram`: the Telegram Bot API client and the long-poll thread.
//! - `test_host`: a fake `Host` for tests only.
pub mod config;
pub mod token;
pub mod policy;
pub mod render;
pub mod grants;
pub mod rate;
pub mod refs;
pub mod commands;
pub mod pairing;
pub mod host;
pub mod engine;
pub mod notify;
pub mod host_live;
pub mod telegram;
#[cfg(test)]
pub mod test_host;

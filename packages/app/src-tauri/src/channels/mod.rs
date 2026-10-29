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
//! - `runtime`: owns the engine, notifier and poller; starts and stops the poller as the settings say.
//! - `setup`: the person's actions (token, code, yes/no, on/off, options, disconnect) and the phone's off switch.
//! - `feed`: what the app tells the chat (candidates, companion replies, the 5 s tick).
//! - `status`: the `TelegramStatus` the Settings page shows.
//! - `live`: builds the real runtime at start-up (`start`) and the global hooks `push` calls.
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
pub mod runtime;
pub mod setup;
pub mod feed;
pub mod status;
mod live;
pub use live::{companion_reply, current, focus_note, listening, offer, offer_answer, pane_status, start};
#[cfg(test)]
pub use live::install_for_test;
#[cfg(test)]
pub mod test_host;
#[cfg(test)]
pub mod fake_bot;

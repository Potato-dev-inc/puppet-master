//! Potato Bot's voice settings: which system voice reads a reply aloud, how fast, and which bots read their
//! replies at all. The webview does the speaking (`speechSynthesis`, the Windows voices, no key, nothing leaves
//! the PC); this file only keeps the choice in `settings.json` next to the other Potato Bot settings.

use serde::{Deserialize, Serialize};

pub const MIN_RATE_PERCENT: u32 = 50;
pub const MAX_RATE_PERCENT: u32 = 200;
pub const DEFAULT_RATE_PERCENT: u32 = 100;
const MAX_VOICE_URI: usize = 300;
const MAX_READ_ALOUD_BOTS: usize = 200;
const MAX_BOT_ID: usize = 120;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct VoiceSettings {
    /// The voice's URI as the webview lists it. Empty: the system's default voice.
    pub voice_uri: String,
    /// Speed in percent of normal (100). Kept between 50 and 200.
    pub rate_percent: u32,
    /// The bots whose new replies are read aloud while the page is visible.
    pub read_aloud_bots: Vec<String>,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        VoiceSettings { voice_uri: String::new(), rate_percent: DEFAULT_RATE_PERCENT, read_aloud_bots: Vec::new() }
    }
}

pub fn clamp_rate_percent(rate: u32) -> u32 {
    rate.clamp(MIN_RATE_PERCENT, MAX_RATE_PERCENT)
}

/// A companion id is short and plain (letters, digits, `-`, `_`). Anything else is dropped.
fn plain_bot_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_BOT_ID && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

impl VoiceSettings {
    /// The values kept in range: the voice name is trimmed and cut, the rate is clamped, and the bot list holds
    /// each plain id once, at most `MAX_READ_ALOUD_BOTS` of them.
    pub fn cleaned(mut self) -> Self {
        self.voice_uri = self.voice_uri.trim().chars().take(MAX_VOICE_URI).collect();
        self.rate_percent = clamp_rate_percent(self.rate_percent);
        let mut seen = std::collections::HashSet::new();
        self.read_aloud_bots = self
            .read_aloud_bots
            .into_iter()
            .map(|id| id.trim().to_string())
            .filter(|id| plain_bot_id(id) && seen.insert(id.clone()))
            .take(MAX_READ_ALOUD_BOTS)
            .collect();
        self
    }
}

//! Potato Bot's voice settings: defaults, the clamps, the bot list, and the trip through `settings.json`.

use std::path::PathBuf;

use super::store::*;
use super::voice::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-potato-voice-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_default_is_the_system_voice_at_normal_speed_with_no_bot_reading_aloud() {
    let v = VoiceSettings::default();
    assert_eq!(v.voice_uri, "");
    assert_eq!(v.rate_percent, 100);
    assert!(v.read_aloud_bots.is_empty());
    assert_eq!(Settings::default().voice, v);
}

#[test]
fn the_speed_is_kept_between_half_and_double() {
    assert_eq!(clamp_rate_percent(0), 50);
    assert_eq!(clamp_rate_percent(49), 50);
    assert_eq!(clamp_rate_percent(130), 130);
    assert_eq!(clamp_rate_percent(9_999), 200);
    let v = VoiceSettings { rate_percent: 1, ..VoiceSettings::default() }.cleaned();
    assert_eq!(v.rate_percent, 50);
}

#[test]
fn the_voice_name_is_trimmed_and_cut_to_its_limit() {
    let v = VoiceSettings { voice_uri: format!("  {}  ", "a".repeat(400)), ..VoiceSettings::default() }.cleaned();
    assert_eq!(v.voice_uri.chars().count(), 300);
    assert!(!v.voice_uri.starts_with(' '));
}

#[test]
fn the_bot_list_keeps_plain_ids_once_and_drops_the_rest() {
    let v = VoiceSettings {
        read_aloud_bots: vec![" mochi ".into(), "mochi".into(), "nimbus-2_b".into(), "".into(), "../etc".into(), "a b".into(), "x".repeat(121)],
        ..VoiceSettings::default()
    }
    .cleaned();
    assert_eq!(v.read_aloud_bots, vec!["mochi".to_string(), "nimbus-2_b".to_string()]);
}

#[test]
fn the_bot_list_is_capped() {
    let ids: Vec<String> = (0..250).map(|n| format!("bot{n}")).collect();
    let v = VoiceSettings { read_aloud_bots: ids, ..VoiceSettings::default() }.cleaned();
    assert_eq!(v.read_aloud_bots.len(), 200);
    assert_eq!(v.read_aloud_bots[0], "bot0");
}

#[test]
fn voice_settings_are_saved_cleaned_and_an_old_file_without_them_reads_as_the_default() {
    let root = temp("trip");
    // A settings.json from before this field existed still loads.
    std::fs::write(settings_path(&root), r#"{"v":1,"brain":"deepseek","defaultPersona":"fish","dailyLimitCents":50,"sandboxActionsPerDay":500}"#).unwrap();
    assert_eq!(load_settings(&root).unwrap().voice, VoiceSettings::default());

    let saved = Settings {
        voice: VoiceSettings { voice_uri: "Microsoft Zira".into(), rate_percent: 999, read_aloud_bots: vec!["mochi".into(), "mochi".into()] },
        ..Settings::default()
    };
    save_settings(&root, &saved).unwrap();
    let back = load_settings(&root).unwrap();
    assert_eq!(back.voice.voice_uri, "Microsoft Zira");
    assert_eq!(back.voice.rate_percent, 200, "saved speed is clamped");
    assert_eq!(back.voice.read_aloud_bots, vec!["mochi".to_string()]);
}

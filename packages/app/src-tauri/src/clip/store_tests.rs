use super::*;
use std::time::Duration;

fn temp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pm-clip-store-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn path_in(dir: &Path, name: &str) -> String {
    dir.join(name).to_string_lossy().to_string()
}

/// The first bytes of a small mp4 file: an `ftyp` box of 24 bytes, then some filler.
fn mp4(fill: u8) -> Vec<u8> {
    let mut bytes = vec![0, 0, 0, 24];
    bytes.extend_from_slice(b"ftypisom");
    bytes.extend_from_slice(&[0, 0, 2, 0]);
    bytes.extend_from_slice(b"isomiso2");
    bytes.extend(std::iter::repeat(fill).take(40));
    bytes
}

/// The first bytes of a small webm file: the EBML header id, then filler.
fn webm(fill: u8) -> Vec<u8> {
    let mut bytes = vec![0x1A, 0x45, 0xDF, 0xA3];
    bytes.extend(std::iter::repeat(fill).take(60));
    bytes
}

/// A valid mp4 header followed by enough bytes to be over the stage cap.
fn mp4_over_cap() -> Vec<u8> {
    let mut bytes = mp4(0);
    bytes.resize(MAX_STAGE_BYTES + 1, 0);
    bytes
}

fn issued_now(plans: &Plans) -> String {
    plans.issue("pane-1")
}

#[test]
fn an_unknown_token_stages_and_saves_nothing() {
    let plans = Plans::default();
    let dir = temp_dir();
    assert!(plans.stage("not-a-token", "mp4", mp4(1)).is_err());
    assert!(plans.save("not-a-token", &path_in(&dir, "a.mp4")).is_err());
    assert!(!dir.join("a.mp4").exists());
}

#[test]
fn an_expired_plan_is_refused() {
    let plans = Plans::default();
    let dir = temp_dir();
    let issued = Instant::now();
    let token = plans.issue_at("pane-1", issued);
    let late = issued + PLAN_TTL + Duration::from_secs(1);
    assert!(plans.stage_at(&token, "mp4", mp4(1), late).is_err());
    assert!(plans.save_at(&token, &path_in(&dir, "a.mp4"), late).is_err());
    assert!(!dir.join("a.mp4").exists());
}

#[test]
fn a_plan_is_usable_for_thirty_minutes() {
    let plans = Plans::default();
    let dir = temp_dir();
    let issued = Instant::now();
    let token = plans.issue_at("pane-1", issued);
    let almost = issued + PLAN_TTL - Duration::from_secs(60);
    plans.stage_at(&token, "mp4", mp4(2), almost).unwrap();
    plans.save_at(&token, &path_in(&dir, "a.mp4"), almost).unwrap();
    assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), mp4(2));
}

#[test]
fn a_video_of_the_wrong_magic_is_refused() {
    let plans = Plans::default();
    let token = issued_now(&plans);
    assert!(plans.stage(&token, "mp4", b"hello world, not a video at all".to_vec()).is_err());
    assert!(plans.stage(&token, "webm", mp4(1)).is_err());
    assert!(plans.stage(&token, "mp4", webm(1)).is_err());
    assert!(plans.stage(&token, "mp4", Vec::new()).is_err());
    // An ftyp box whose size says more than the file has.
    let mut short = mp4(1);
    short[3] = 200;
    assert!(plans.stage(&token, "mp4", short).is_err());
    // An ftyp box whose size is smaller than a box header.
    let mut tiny = mp4(1);
    tiny[..4].copy_from_slice(&[0, 0, 0, 4]);
    assert!(plans.stage(&token, "mp4", tiny).is_err());
    assert_eq!(plans.staged_len(&token), None);
}

#[test]
fn a_video_of_an_unknown_kind_is_refused() {
    let plans = Plans::default();
    let token = issued_now(&plans);
    assert!(plans.stage(&token, "avi", mp4(1)).is_err());
    assert!(plans.stage(&token, "MP4", mp4(1)).is_err());
    assert!(plans.stage(&token, "", mp4(1)).is_err());
    assert_eq!(plans.staged_len(&token), None);
}

#[test]
fn a_valid_mp4_and_a_valid_webm_are_staged() {
    let plans = Plans::default();
    let token = issued_now(&plans);
    assert_eq!(plans.stage(&token, "mp4", mp4(3)).unwrap(), mp4(3).len() as u64);
    assert_eq!(plans.stage(&token, "webm", webm(4)).unwrap(), webm(4).len() as u64);
    assert_eq!(plans.staged_len(&token), Some(webm(4).len()));
}

#[test]
fn a_video_over_the_stage_cap_is_refused() {
    let plans = Plans::default();
    let token = issued_now(&plans);
    let error = plans.stage(&token, "mp4", mp4_over_cap()).unwrap_err();
    assert!(error.contains("too big"), "{error}");
    assert_eq!(plans.staged_len(&token), None);
}

#[test]
fn an_oversized_webm_is_refused() {
    let plans = Plans::default();
    let token = issued_now(&plans);
    let mut bytes = webm(5);
    bytes.resize(MAX_STAGE_BYTES + 1, 0);
    assert!(plans.stage(&token, "webm", bytes).is_err());
    assert_eq!(plans.staged_len(&token), None);
}

#[test]
fn a_refused_stage_keeps_the_earlier_video() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(6)).unwrap();
    assert!(plans.stage(&token, "mp4", webm(6)).is_err());
    plans.save(&token, &path_in(&dir, "a.mp4")).unwrap();
    assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), mp4(6));
}

#[test]
fn a_new_stage_replaces_the_staged_video() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(7)).unwrap();
    plans.stage(&token, "mp4", mp4(8)).unwrap();
    plans.save(&token, &path_in(&dir, "a.mp4")).unwrap();
    assert_eq!(std::fs::read(dir.join("a.mp4")).unwrap(), mp4(8));
}

#[test]
fn save_writes_once_and_uses_the_staged_bytes() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(9)).unwrap();
    let written = plans.save(&token, &path_in(&dir, "one.mp4")).unwrap();
    assert_eq!(std::fs::read(written).unwrap(), mp4(9));
    // The bytes are used up: a second save with no new stage writes nothing.
    let again = plans.save(&token, &path_in(&dir, "two.mp4")).unwrap_err();
    assert!(again.contains("not ready"), "{again}");
    assert!(!dir.join("two.mp4").exists());
    // The plan stays, so "save another copy" only stages again.
    plans.stage(&token, "mp4", mp4(9)).unwrap();
    plans.save(&token, &path_in(&dir, "two.mp4")).unwrap();
    assert_eq!(std::fs::read(dir.join("two.mp4")).unwrap(), mp4(9));
}

#[test]
fn save_before_any_stage_writes_nothing() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    assert!(plans.save(&token, &path_in(&dir, "a.mp4")).is_err());
    assert!(!dir.join("a.mp4").exists());
}

#[test]
fn a_refused_path_keeps_the_staged_video_for_another_try() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(10)).unwrap();
    assert!(plans.save(&token, &path_in(&dir, "notes.txt")).is_err());
    assert!(plans.save(&token, "relative.mp4").is_err());
    assert!(plans.save(&token, &path_in(&dir, "../escape.mp4")).is_err());
    assert_eq!(plans.staged_len(&token), Some(mp4(10).len()));
    plans.save(&token, &path_in(&dir, "ok.mp4")).unwrap();
    assert_eq!(std::fs::read(dir.join("ok.mp4")).unwrap(), mp4(10));
}

#[test]
fn the_saved_extension_must_match_the_staged_kind() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(11)).unwrap();
    let webm_name = plans.save(&token, &path_in(&dir, "a.webm")).unwrap_err();
    assert!(webm_name.contains(".mp4"), "{webm_name}");
    assert!(plans.save(&token, &path_in(&dir, "a.html")).is_err());
    assert!(!dir.join("a.webm").exists());
    assert!(!dir.join("a.html").exists());
    plans.save(&token, &path_in(&dir, "a.mp4")).unwrap();
}

#[test]
fn a_webm_saves_only_as_webm() {
    let plans = Plans::default();
    let dir = temp_dir();
    let token = issued_now(&plans);
    plans.stage(&token, "webm", webm(12)).unwrap();
    let error = plans.save(&token, &path_in(&dir, "a.mp4")).unwrap_err();
    assert!(error.contains(".webm"), "{error}");
    assert!(!dir.join("a.mp4").exists());
    plans.save(&token, &path_in(&dir, "a.webm")).unwrap();
    assert_eq!(std::fs::read(dir.join("a.webm")).unwrap(), webm(12));
}

#[test]
fn only_the_newest_four_plans_are_kept() {
    let plans = Plans::default();
    let dir = temp_dir();
    let tokens: Vec<String> = (0..5).map(|n| plans.issue(&format!("pane-{n}"))).collect();
    assert_eq!(plans.count(), KEEP_PLANS);
    assert!(plans.stage(&tokens[0], "mp4", mp4(13)).is_err());
    assert_eq!(plans.pane_of(&tokens[0]), None);
    plans.stage(&tokens[4], "mp4", mp4(13)).unwrap();
    plans.save(&tokens[4], &path_in(&dir, "newest.mp4")).unwrap();
    assert_eq!(plans.pane_of(&tokens[4]).as_deref(), Some("pane-4"));
}

#[test]
fn an_existing_link_is_refused() {
    let plans = Plans::default();
    let dir = temp_dir();
    let target = dir.join("target.mp4");
    std::fs::write(&target, "keep me").unwrap();
    let link = dir.join("link.mp4");
    #[cfg(windows)]
    let made = std::os::windows::fs::symlink_file(&target, &link);
    #[cfg(unix)]
    let made = std::os::unix::fs::symlink(&target, &link);
    if made.is_err() {
        // Making a link needs a privilege on some Windows set-ups; nothing to check there.
        return;
    }
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(14)).unwrap();
    let error = plans.save(&token, &link.to_string_lossy()).unwrap_err();
    assert!(error.contains("link"), "{error}");
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep me");
}

#[test]
fn a_failed_write_keeps_the_staged_video() {
    let plans = Plans::default();
    let dir = temp_dir();
    let locked = dir.join("locked.mp4");
    std::fs::write(&locked, "old").unwrap();
    let mut readonly = std::fs::metadata(&locked).unwrap().permissions();
    readonly.set_readonly(true);
    std::fs::set_permissions(&locked, readonly.clone()).unwrap();
    let token = issued_now(&plans);
    plans.stage(&token, "mp4", mp4(15)).unwrap();
    let error = plans.save(&token, &path_in(&dir, "locked.mp4"));
    // Put the write permission back before any assertion, so the temp folder stays usable.
    readonly.set_readonly(false);
    std::fs::set_permissions(&locked, readonly).unwrap();
    if error.is_ok() {
        // The file system let the write through (some set-ups ignore the read-only flag); nothing to check here.
        return;
    }
    assert!(error.unwrap_err().contains("Could not write"));
    assert_eq!(plans.staged_len(&token), Some(mp4(15).len()));
    plans.save(&token, &path_in(&dir, "locked.mp4")).unwrap();
    assert_eq!(std::fs::read(&locked).unwrap(), mp4(15));
}

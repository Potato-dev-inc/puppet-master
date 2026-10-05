//! Operations tests: acceptance.

use super::*;

#[test]
fn acceptance_criteria_are_evaluated_on_completion() {
    let dir = temp();
    let op = delegate_work(request(&dir, "accept"), |_| Ok(())).unwrap();
    mark_operation_finished(
        &dir.to_string_lossy(),
        &op.operation_id,
        Some("behavior works as expected".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let finished = get_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(
        finished.acceptance_status,
        AcceptanceStatus::Passed,
        "criteria substring should match result"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn acceptance_text_mismatch_is_not_checked_not_failed() {
    let dir = temp();
    let mut req = request(&dir, "accept-soft");
    req.acceptance_criteria = Some(vec!["behavior works".into()]);
    let op = delegate_work(req, |_| Ok(())).unwrap();
    mark_operation_finished(
        &dir.to_string_lossy(),
        &op.operation_id,
        Some("The backend is a Rust/Tauri crate with src and Cargo.toml.".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let finished = get_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(
        finished.acceptance_status,
        AcceptanceStatus::NotChecked,
        "paraphrased answers must not hard-fail text criteria"
    );
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn acceptance_file_exists_criterion() {
    let dir = temp();
    fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("proof.txt");
    fs::write(&marker, "ok").unwrap();
    let mut req = request(&dir, "accept-file");
    req.acceptance_criteria = Some(vec!["file_exists:proof.txt".into()]);
    let op = delegate_work(req, |_| Ok(())).unwrap();
    mark_operation_finished(
        &dir.to_string_lossy(),
        &op.operation_id,
        Some("done".into()),
        ResultCapture::Authoritative,
        true,
    )
    .unwrap();
    let finished = get_operation(&dir.to_string_lossy(), &op.operation_id).unwrap();
    assert_eq!(finished.acceptance_status, AcceptanceStatus::Passed);
    let _ = fs::remove_dir_all(dir);
}

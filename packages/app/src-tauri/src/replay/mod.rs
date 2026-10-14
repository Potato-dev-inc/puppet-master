//! Deterministic replay of recorded PTY output (test-only).
//!
//! Every live bug becomes a failing test before it is fixed: record the pane (or build the cast
//! from captured screens), replay it through the code that decides things, and snapshot what that
//! code decided over time. A fix then changes the snapshot in a diff a reviewer can read.
//!
//! - [`cast`]: the corpus format, asciicast v2 plus two small extensions, and its loader.
//! - [`pane::ReplayPane`]: a pane fed by a cast under VIRTUAL time (no sleeps). It runs the same
//!   screen model and status steps as the PTY reader, and is a `DeliveryIo` the delivery protocol
//!   can type into.
//! - [`startup`]: the startup-gate wait (folder trust, sign-in), stepped screen by screen.
//! - [`receipt`]: the end-of-turn summary and its checkpoint settle, on virtual time.
//! - `corpus_tests`: one test per cast in `tests/corpus/`, asserting the correct decision and
//!   snapshotting the decision timeline with `insta` into `tests/corpus/snapshots/`.
//!
//! ## Add a cast from a live capture
//!
//! 1. Capture the screens with their times: a flight recording (`.cast`) once that exists, or poll
//!    `GET /panes/{id}/buffer` every 100 ms and keep the rows that changed (see
//!    `D:\pm-night\r9\live\a1.mjs`). Keep the real text; do not tidy it.
//! 2. Write a spec and build the cast: `node tests/corpus/tools/screens-to-cast.mjs spec.json
//!    tests/corpus/<agent>/<version>/<scenario>.cast` (the spec format is in the script's header).
//!    Put the bug, the fix commit and where the screens came from in the header's `pm` object.
//!    If the code under test types into the pane (delivery), add `react` events so the cast answers
//!    its keys the way the agent did.
//! 3. Add a test to `corpus_tests.rs`: load the cast, replay it with the driver that matches the
//!    decision (`deliver_prompt_traced` for delivery, `startup::replay_startup`, `receipt`, or
//!    just `run_to_end` for status), assert the CORRECT outcome, and snapshot the timeline.
//!    `INSTA_UPDATE=always cargo test --lib replay` writes the snapshot; read it before committing.
//! 4. Prove it catches the bug: revert the fix locally and see the new test fail, then restore it.
//!
//! The drivers call only functions the app itself calls, and `ReplayPane::output` mirrors the PTY
//! reader's status steps (a test fails when the reader's call order changes).

pub mod cast;
pub mod pane;
pub mod receipt;
pub mod startup;


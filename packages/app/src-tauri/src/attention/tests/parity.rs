//! The shared corpus (`packages/shared/fixtures/attention/*.json`) is also run by vitest against
//! `deriveNeedsYou` (`lib/attention.corpus.test.ts`). Same files, same expected output: neither
//! implementation can drift from the other without a test failing.

use crate::attention::derive::derive;
use crate::attention::model::Facts;
use serde_json::Value;
use std::path::PathBuf;

pub fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../shared/fixtures/attention")
}

pub fn corpus() -> Vec<(String, Value)> {
    let mut files: Vec<_> = std::fs::read_dir(corpus_dir())
        .expect("the shared corpus directory")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    files.sort();
    files
        .into_iter()
        .map(|p| {
            let text = std::fs::read_to_string(&p).unwrap();
            (p.file_name().unwrap().to_string_lossy().into_owned(), serde_json::from_str(&text).unwrap())
        })
        .collect()
}

/// The same files also say what COUNTS: the TS selector `blockingItems` and the Rust store must
/// agree on every case (Ask-bar answers are ready, one key counts once, a pane has one pane-state item).
#[test]
fn the_store_counts_what_the_ts_selector_counts() {
    use crate::attention::model::{Lane, State};
    use crate::attention::store::Store;
    for (file, case) in corpus() {
        let facts: Facts = serde_json::from_value(case["input"].clone()).unwrap();
        let mut store = Store::default();
        store.recompute(&facts, 1_000);
        let mut counted: Vec<String> = store
            .snapshot()
            .items
            .iter()
            .filter(|i| i.state == State::Open && i.lane == Lane::Blocking)
            .map(|i| i.key.clone())
            .collect();
        counted.sort();
        let expected: Vec<String> = serde_json::from_value(case["blocking"].clone()).unwrap();
        assert_eq!(counted, expected, "{file}: {}", case["note"]);
        assert_eq!(store.counts().blocking as usize, expected.len(), "{file}: the number");
    }
}

#[test]
fn attention_corpus_matches_ts() {
    let cases = corpus();
    assert!(cases.len() >= 20, "the corpus went missing: {} cases", cases.len());
    for (file, case) in cases {
        let facts: Facts = serde_json::from_value(case["input"].clone()).unwrap_or_else(|e| panic!("{file}: input: {e}"));
        let got = serde_json::to_value(derive(&facts)).unwrap();
        assert_eq!(got, case["expected"], "{file}: {}", case["note"]);
    }
}

//! Package check, lookups: what a public registry says about a package an agent wants to install.
//!
//! The offline part (which packages a command names, look-alike names) is in
//! `prompt_risk/packages/`. This module calls the registries (npm, PyPI, crates.io, RubyGems, Go),
//! caches the answers in memory, and turns the facts into the card's sentences.
//!
//! Invariants (SPEC-n33 section 4): S1 no answer, block or write path. S2 only the package name is
//! sent (plus the version a Go mirror listed); headers are User-Agent and Accept only. S8 network facts
//! never change the risk level, the buttons or the focus. S10 Tauri commands only: no bridge route,
//! no MCP tool.
//!
//! `model.rs` holds the numbers and shapes. Every sentence the card shows is in `prompt_risk/packages/words.rs`.

// Phase 1 stubs: the modules below are filled in Phase 2 and wired in Phases 3 and 4.
#![allow(dead_code)]

pub mod model;
pub mod http;
pub mod cache;
pub mod settings;
pub mod lines;
pub mod private;
pub mod registries;
pub mod live;

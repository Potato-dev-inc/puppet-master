//! Declarative agent definitions (SPEC 7.1): TOML schema, strict parse and the pure validator.
//!
//! Nothing here does I/O or registers anything (VAL-8); loading and registration come later.

// Consumers (loader, template, selectors) land in later phases.
#![allow(dead_code)]

pub mod approvals;
pub mod check;
pub mod fingerprint;
pub mod hooks;
pub mod loader;
pub mod output;
pub mod removal;
pub mod resolve;
pub mod schema;
pub mod screen;
pub mod select;
pub mod service;
pub mod sources;
pub mod template;
pub mod validate;
pub mod watch;
pub mod wizard;

mod profile;

#[cfg(test)]
mod tests;

/// Parse, validate (as a user definition) and build; every problem becomes one message.
#[cfg(test)]
pub(crate) fn build_for_test(text: &str) -> Result<DeclarativeProfile, String> {
    let def = parse_definition(text).map_err(|e| e.to_string())?;
    let ctx = ValidationContext {
        source: Source::User,
        code_ids: &["claude", "codex", "cursor_agent", "opencode", "shell"],
        raw_len: text.len(),
    };
    let errors = validate(&def, &ctx);
    if !errors.is_empty() {
        return Err(errors.iter().map(|e| format!("{} {}", e.code, e.path)).collect::<Vec<_>>().join("; "));
    }
    DeclarativeProfile::new(def).map_err(|e| e.to_string())
}

// `tui::declarative::kilo` (phase T21 verification filter).
#[cfg(test)]
#[path = "tests/kilo.rs"]
mod kilo;

#[allow(unused_imports)]
pub use schema::{parse_definition, DefError, Definition, SUPPORTED_SCHEMA_VERSIONS};
#[allow(unused_imports)]
pub use validate::{validate, Source, ValidationContext};
#[allow(unused_imports)]
pub use profile::{DeclarativeProfile, DefinitionOrigin};

// `tui::declarative::expect` (phase T27 verification filter).
#[cfg(test)]
#[path = "tests/expect.rs"]
mod expect;

// `tui::declarative::parity_claude` (phase T28 verification filter).
#[cfg(test)]
#[path = "tests/parity_claude.rs"]
mod parity_claude;

// `tui::declarative::generic` (phase T30 verification filter).
#[cfg(test)]
#[path = "tests/generic.rs"]
mod generic;

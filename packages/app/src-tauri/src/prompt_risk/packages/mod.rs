//! Package check, offline part: which packages one command installs, and the look-alike name check.
//!
//! Invariants (SPEC-n33 section 4):
//! - S1: nothing here answers, blocks or delays a prompt. There is no write path: no key or text is
//!   sent to a pane. `prompt_id` and `choices` never depend on these fields.
//! - S7: facts only. Every sentence is in `words.rs`; nothing reassuring is ever shown.
//! - S8: these fields only add facts. The look-alike finding is the one thing that can raise a level,
//!   and it is computed before the card first paints.
//! - Detection is by package tool (`npm`, `pip`, `cargo`, ...), never by agent name.
//!
//! Registry lookups (network, cache, card rows) live in `package_check/`, not here.

// Phase 1 stubs: `parse`, `lookalike` and `popular` are filled in Phase 2, and `extract` gets its
// callers in Phase 3. Until then the items below are only used by tests.
#![allow(dead_code)]

pub mod model;
pub mod words;
pub(crate) mod lookalike;
pub(crate) mod parse;
pub(crate) mod popular;

use std::collections::HashSet;

use crate::redact;
use model::{Ecosystem, PackageItem, PackageList, Source, MAX_PACKAGES};

/// The packages a command installs, ready for the card (SPEC-n33 section 3.2, steps 1 to 7).
///
/// Pure and never panics. `last_may_be_cut` is true when the command may have been cut at the edge
/// of the screen: its last name is then marked `cut` and is not checked.
pub fn extract(command: &str, last_may_be_cut: bool) -> PackageList {
    let mut list = PackageList::default();
    let mut seen: HashSet<(Ecosystem, String)> = HashSet::new();
    let mut items: Vec<PackageItem> = Vec::new();

    for raw in parse::parse(command) {
        // 1. The same package twice is listed once (the first spelling wins).
        let norm = raw.ecosystem.normalize(&raw.name);
        if !seen.insert((raw.ecosystem, norm)) {
            continue;
        }
        // 2. A name that looks like a secret is counted, never shown or checked.
        if redact::strict(&raw.name).as_str() != raw.name {
            list.hidden += 1;
            continue;
        }
        // 3. A registry name that fails the name rules is not a plain package name.
        let source = if raw.source == Source::Registry && !raw.ecosystem.valid_name(&raw.name) {
            Source::NotPlain
        } else {
            raw.source
        };
        items.push(PackageItem {
            ecosystem: raw.ecosystem,
            tool: raw.tool,
            name: raw.name,
            spec: raw.spec,
            source,
            global: raw.global,
            runs_now: raw.runs_now,
            lookalike: None,
            popular: false,
            cut: false,
            lookup: false,
            first: String::new(),
            look: false,
        });
    }

    // 4. Keep the first MAX_PACKAGES; count the rest.
    if items.len() > MAX_PACKAGES {
        list.more = (items.len() - MAX_PACKAGES) as u32;
        items.truncate(MAX_PACKAGES);
    }

    // 5. The last name of a possibly cut command is cut.
    if last_may_be_cut {
        if let Some(last) = items.last_mut() {
            last.cut = true;
        }
    }

    for item in items.iter_mut() {
        let norm = item.ecosystem.normalize(&item.name);
        // 6. Offline list hit, or else a look-alike name (never for a cut name).
        if item.source == Source::Registry && !item.cut && item.ecosystem.has_popular_list() {
            item.popular = popular::rank(item.ecosystem, &norm).is_some();
            item.lookalike = if item.popular { None } else { lookalike::check(item.ecosystem, &norm) };
        }
        // 7. Whether a registry lookup runs, and the first-paint text.
        item.lookup = item.source == Source::Registry && item.ecosystem.looks_up() && !item.cut;
        let (first, look) = words::first_line(item);
        item.first = first;
        item.look = look;
    }

    list.items = items;
    list
}

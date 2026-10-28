//! The built-in starter crews: three TOML files compiled into the app. A starter is never written into the
//! profile's app data. "Copy to My crews" in the Start sheet is the only way one becomes a saved crew.
//! A starter is named `starter:<id>`, so its name never matches a file of ours (a file slug has no colon).

use super::model::{parse, Crew};
use super::store::{self, Summary};
use super::validate::AgentState;

pub const PREFIX: &str = "starter:";

/// (id, file text), in the order the Start sheet lists them.
const SOURCES: [(&str, &str); 3] = [
    ("builder-reviewer", include_str!("starters/builder-reviewer.toml")),
    ("fix-the-tests", include_str!("starters/fix-the-tests.toml")),
    ("research-write", include_str!("starters/research-write.toml")),
];

/// Every starter as (slug, crew).
pub fn all() -> Vec<(String, Crew)> {
    SOURCES.iter().filter_map(|(id, text)| parse(text).ok().map(|crew| (format!("{PREFIX}{id}"), crew))).collect()
}

/// The starter a slug names (`None` for any other slug, or an unknown starter).
pub fn lookup(slug: &str) -> Option<Crew> {
    let id = slug.strip_prefix(PREFIX)?;
    SOURCES.iter().find(|(known, _)| *known == id).and_then(|(_, text)| parse(text).ok())
}

/// The starters as the list shows them.
pub fn summaries(agent_state: &dyn Fn(&str) -> AgentState) -> Vec<Summary> {
    all().iter().map(|(slug, crew)| store::summary(slug, crew, agent_state)).collect()
}

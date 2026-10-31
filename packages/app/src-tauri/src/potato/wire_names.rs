//! Names that are fixed by the brain package's wire format and happen to be the names of other things.
//! They live in this one file so the guard that keeps generic code free of agent names
//! (`generic_code_never_names_an_agent`) can name this single file as its reasoned exception instead of
//! weakening the scan:
//!
//! - the values of `FISHD_PROVIDER` the brain understands. They are the brain's own provider ids, not
//!   Puppet Master agents;
//! - the name of the long-poll position (`cursor`) in the inbox protocol. It is a page position, not
//!   an agent.

pub const DEEPSEEK: &str = "deepseek";
/// The brain package spells its Go plan provider with the name of the plan's maker.
pub const GO: &str = "opencode";
/// The inbox query parameter and the key of the answer: where the brain is in the inbox.
pub const POLL_POSITION: &str = "cursor";

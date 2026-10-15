//! Where "something happened" comes from: local git (pushes), `gh` (the PR for a pushed branch),
//! and the GitHub poller (pull requests opened, merged or closed; CI red or green).

pub mod git_refs;
pub mod github;
pub mod pr;

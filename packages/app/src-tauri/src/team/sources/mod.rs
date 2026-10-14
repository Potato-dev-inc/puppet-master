//! Where "something happened" comes from. v0: local git (pushes) and `gh` (the PR for a pushed
//! branch). v1 adds a GitHub polling source (merges, CI, reviews) and background fetch.

pub mod git_refs;
pub mod pr;

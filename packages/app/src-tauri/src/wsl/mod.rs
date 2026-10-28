//! Linux Inside (SPEC-n33): run an agent inside a WSL 2 distro. Only the pure folder-path mapping lives here so far
//! (`path`, WSL-5); the model, launch, probe and settings modules come in their own tasks.

pub mod path;

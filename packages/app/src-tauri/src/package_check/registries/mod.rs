//! One module per public registry (SPEC-n33 section 3.6). Each has `fn lookup(f: &dyn Fetcher, name: &str)
//! -> Outcome`. Phase 1 stubs; filled by PC-5 (npm, PyPI) and PC-6 (crates, gems, Go).

pub mod npm;
pub mod pypi;
pub mod crates;
pub mod gems;
pub mod go;

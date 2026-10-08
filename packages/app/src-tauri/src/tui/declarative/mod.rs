//! Declarative agent definitions (SPEC 7.1): TOML schema, strict parse and the pure validator.
//!
//! Nothing here does I/O or registers anything (VAL-8); loading and registration come later.

// Consumers (loader, template, selectors) land in later phases.
#![allow(dead_code)]

pub mod resolve;
pub mod schema;
pub mod template;
pub mod validate;

#[cfg(test)]
mod tests;

#[allow(unused_imports)]
pub use schema::{parse_definition, DefError, Definition, SUPPORTED_SCHEMA_VERSIONS};
#[allow(unused_imports)]
pub use validate::{validate, Source, ValidationContext};

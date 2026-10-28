//! Agent Store: a catalog of terminal AI agents, one folder per agent, embedded in the app.
//!
//! The Store is data plus checks. Nothing here types into a pane, reads a credential or makes a
//! network request of its own. `entry` parses and checks the card data (`entry.toml`); the other
//! modules are filled in by later tasks.

// Nothing calls this module yet; the Store commands wire it in later. Remove this line then.
#![allow(dead_code)]

pub mod add;
pub mod catalog;
pub mod check;
pub mod embedded;
pub mod entry;
pub mod record;
pub mod state;
pub mod uninstall;

//! The live entry of the pick: reads the Scores rows, the registry, the accounts and the limits of
//! the folder, then calls `compose::pick`. Scaffold placeholder: no sources are read yet.

use super::compose;
use super::model::{Pick, Usual};

/// The pick for a folder. Scaffold placeholder: the answer has no choice.
pub fn pick_for_folder(_folder: &str) -> Result<Pick, String> {
    Ok(compose::pick(&[], &Usual { preferred: None, last: None }, &|_| String::new()))
}

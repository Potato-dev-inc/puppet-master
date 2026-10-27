//! The `dir` template of the table: where the transcript was found, written with `{home}`, `{cwd}` and the
//! `{cwd_*}` placeholders so it holds for any project folder, not only the wizard's temporary one.

use std::path::{Component, Path};

use crate::session_chat::{cwd_placeholders, plain_path};

/// `dir` as a template, or `None` when it is neither under the pane's folder nor under the home folder (the table's
/// own rule: nothing else is ever read) or a segment of it has braces.
pub fn dir_template(dir: &Path, cwd: &Path, home: Option<&Path>) -> Option<String> {
    let dir = Path::new(&plain_path(&dir.to_string_lossy())).to_path_buf();
    let cwd = Path::new(&plain_path(&cwd.to_string_lossy())).to_path_buf();
    let (root, rest) = if let Ok(rest) = dir.strip_prefix(&cwd) {
        ("{cwd}", rest.to_path_buf())
    } else {
        let home = home.map(|h| Path::new(&plain_path(&h.to_string_lossy())).to_path_buf())?;
        ("{home}", dir.strip_prefix(&home).ok()?.to_path_buf())
    };
    let known = cwd_placeholders(&cwd);
    let mut out = root.to_string();
    for part in rest.components() {
        let Component::Normal(name) = part else { return None };
        let name = name.to_string_lossy();
        if name.contains(['{', '}', '/', '\\']) {
            return None;
        }
        out.push('/');
        match known.iter().find(|(_, value)| !value.is_empty() && value.as_str() == name) {
            Some((placeholder, _)) => out.push_str(&format!("{{{placeholder}}}")),
            None => out.push_str(&name),
        }
    }
    Some(out)
}

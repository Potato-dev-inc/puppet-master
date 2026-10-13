//! APR-2: SHA-256 over canonical JSON of the executable-relevant parts of a definition.
//!
//! Included: `binary`, every headless/tui argv template and conditional segment, the stdin
//! template, `env`, `allow_interpreter`, the model probe command, `mcp_host` (it writes host
//! config), the source, the canonical project path (project definitions) and the file's absolute
//! path. Excluded on purpose (APR-5): label, icon, description, regex tables, parser selectors,
//! context. Canonical JSON = `serde_json` maps (sorted keys) with no whitespace, so formatting and
//! comments in the TOML never matter.

use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::schema::{Cwd, Definition, OptionalSegment};
use super::sources::source_name;
use super::validate::Source;
use crate::standby::key::canonical_project;

fn segments(list: &[OptionalSegment]) -> Value {
    Value::Array(list.iter().map(|s| json!({ "when": s.when, "args": s.args })).collect())
}

/// The value that is hashed; exposed so tests (and the approval dialog) can show what counts.
pub fn canonical_value(def: &Definition, source: Source, project: Option<&str>, file: &Path) -> Value {
    let headless = def.headless.as_ref().map(|h| {
        json!({
            "argv": h.argv,
            "task_flag": h.task_flag,
            "task_separator": h.task_separator,
            "cwd": match h.cwd { Cwd::Project => "project", Cwd::Inherit => "inherit" },
            "optional": segments(&h.optional),
            "stdin": h.stdin.as_ref().map(|s| s.template.clone()),
        })
    });
    let tui = def.tui.as_ref().map(|t| {
        json!({
            "argv": t.argv,
            "optional": segments(&t.optional),
            "read_only_argv": t.read_only_argv,
        })
    });
    let mut value = json!({
        "v": 1,
        "source": source_name(source),
        "project": project.map(canonical_project),
        "file": canonical_project(&file.to_string_lossy()),
        "allow_interpreter": def.agent.allow_interpreter,
        "binary": {
            "names": def.binary.names,
            "windows_extensions": def.binary.windows_extensions,
            "shim_native_globs": def.binary.shim_native_globs,
            "allow_cmd_shim": def.binary.allow_cmd_shim,
            "version_args": def.binary.version_args,
        },
        "env": { "set": def.env.set, "unset": def.env.unset },
        "headless": headless,
        "tui": tui,
        "model_probe": def.model.probe_argv,
        "mcp_host": def.mcp_host.as_ref().map(|m| json!({
            "config_path": m.config_path,
            "key_path": m.key_path,
            "entry": serde_json::to_value(&m.entry).unwrap_or(Value::Null),
            "project_config_path": m.project_config_path,
            "entry_http": m.entry_http.as_ref().and_then(|t| serde_json::to_value(t).ok()),
        })),
    });
    // Present only when declared, so definitions without `[accounts]` keep their fingerprint.
    if let Some(a) = &def.accounts {
        value["accounts"] = json!({ "home_env": a.home_env, "extra_env": a.extra_env, "login_hint": a.login_hint });
    }
    value
}

/// `sha256:<hex>` of `canonical_value`.
pub fn fingerprint(def: &Definition, source: Source, project: Option<&str>, file: &Path) -> String {
    let canonical = canonical_value(def, source, project, file).to_string();
    let digest = Sha256::digest(canonical.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("sha256:{hex}")
}

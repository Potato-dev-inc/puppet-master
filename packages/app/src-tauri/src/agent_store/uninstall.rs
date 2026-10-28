//! Uninstall plan for a program the Store installed.
//!
//! Two rules keep it safe. The plan comes from the CURRENT catalog entry and must name the same
//! method and package that the Store recorded at install time (`plan_uninstall`), so a catalog
//! change never points a removal at another program. The run goes through the same confirm and
//! visible-pane path as an install (`agent_install::start_plan`).

use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::Value;

use crate::agent_install::plan::{plan_uninstall, Plan, Tools};
use crate::agent_install::{plan_view, start_plan};
use crate::agent_store::catalog::{self, CatalogEntry};
use crate::agent_store::record::{self, InstallRecord};
use crate::pty::PaneRegistry;
use crate::tui::install::InstallSpec;

/// Plain words for "the Store did not install this, so it has no removal here".
const NOT_STORE_INSTALL: &str = "The Store did not install this program, so Puppet Master cannot remove it. Remove it where you installed it.";

/// `agent_store_uninstall_plan`: the removal the Store would run. Runs nothing.
pub fn plan(id: &str) -> Result<Value, String> {
    let entry = entry_for(id)?;
    let spec = catalog::install_spec(id);
    let plan = uninstall_plan(entry, spec.as_ref(), install_record(id).as_ref(), &Tools::detect());
    Ok(plan_view(&plan, None))
}

/// `agent_store_uninstall_run`: runs one confirmed removal step in a visible pane. The plan is
/// rebuilt here, and `shown_command` must equal the step's command in that plan.
pub fn run(
    app: &tauri::AppHandle,
    registry: &Arc<Mutex<PaneRegistry>>,
    id: &str,
    step: usize,
    shown_command: &str,
    acknowledged_risk: bool,
) -> Result<Value, String> {
    let entry = entry_for(id)?;
    let spec = catalog::install_spec(id);
    let plan = uninstall_plan(entry, spec.as_ref(), install_record(id).as_ref(), &Tools::detect());
    start_plan(app, registry, &plan, step, shown_command, acknowledged_risk, "Uninstall")
}

/// The uninstall plan for one catalog entry. Pure: the caller passes the current spec, the Store's
/// install record for this agent (`None` when the Store never installed it) and the tools here.
pub fn uninstall_plan(
    entry: &CatalogEntry,
    spec: Option<&InstallSpec>,
    install: Option<&InstallRecord>,
    tools: &Tools,
) -> Plan {
    let id = entry.entry.store.id.as_str();
    let label = entry.label();
    match install {
        Some(install) => plan_uninstall(id, &label, spec, &install.method, &install.package, tools),
        None => Plan {
            agent_id: id.into(),
            label,
            steps: Vec::new(),
            blocked: Some(NOT_STORE_INSTALL.into()),
        },
    }
}

fn entry_for(id: &str) -> Result<&'static CatalogEntry, String> {
    catalog::get(id).ok_or_else(|| format!("INSTALL_UNKNOWN_AGENT: {id} is not an agent Puppet Master knows"))
}

/// The Store's install record for `id`, read from the record file now (`None` when there is none).
fn install_record(id: &str) -> Option<InstallRecord> {
    record::load(&record::default_path()).agents.remove(id)?.install
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A store entry `id` with a definition that names one npm package, loaded like the shipped catalog.
    fn store_entry(id: &str) -> CatalogEntry {
        let entry = format!(
            r#"schema_version = 1

[store]
id = "{id}"
tagline = "A test agent."
tier = "starter"
builtin = false
categories = ["general"]
status = "active"
first_listed = "2026-10-28"

[vendor]
name = "Test Vendor"
homepage = "https://example.com/{id}"
license = "Apache-2.0"
license_kind = "open_source"

[sign_in]
kind = "none"
summary = "No sign-in needed."

[checked]
date = "2026-10-28"
version = "1.0.0"
how = "vendor_docs"
windows_tested = false
"#
        );
        let definition = format!(
            "schema_version = 1\n\n[agent]\nid = \"{id}\"\nlabel = \"Test {id}\"\n\n[binary]\nnames = [\"{id}\"]\n\n\
             [install.windows]\nsource = \"https://example.com/{id}\"\nnpm = \"test-{id}-cli\"\n"
        );
        let files = [
            (format!("agents/{id}/entry.toml"), entry),
            (format!("agents/{id}/definition.toml"), definition),
        ];
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let loaded = catalog::load(&refs);
        assert!(loaded.problems.is_empty(), "{:?}", loaded.problems);
        loaded.entries.into_iter().next().expect("one entry")
    }

    fn acme_spec() -> InstallSpec {
        InstallSpec {
            source: "https://example.com/acme".into(),
            npm: Some("test-acme-cli".into()),
            ..Default::default()
        }
    }

    fn record(method: &str, package: &str) -> InstallRecord {
        InstallRecord {
            method: method.into(),
            package: package.into(),
            version: "1.0.0".into(),
            date: "2026-10-28".into(),
        }
    }

    fn tools() -> Tools {
        Tools { winget: Some("winget".into()), present: Default::default() }
    }

    #[test]
    fn a_store_install_is_removed_by_the_same_method_and_package() {
        let entry = store_entry("acme");
        let plan = uninstall_plan(&entry, Some(&acme_spec()), Some(&record("npm", "test-acme-cli")), &tools());
        assert!(plan.blocked.is_none(), "{:?}", plan.blocked);
        assert_eq!(plan.label, "Test acme", "the definition's name, not the vendor's");
        assert_eq!(plan.steps.len(), 1, "one step, never a prerequisite");
        assert_eq!(plan.steps[0].command, "npm uninstall -g test-acme-cli");
        let view = plan_view(&plan, None);
        assert_eq!(view["available"], true);
    }

    #[test]
    fn uninstall_is_blocked_without_a_store_record() {
        let entry = store_entry("acme");
        let plan = uninstall_plan(&entry, Some(&acme_spec()), None, &tools());
        assert!(plan.steps.is_empty());
        assert!(plan.blocked.as_deref().unwrap_or("").contains("did not install"));
        assert_eq!(plan_view(&plan, None)["available"], false);
    }

    #[test]
    fn uninstall_is_blocked_when_the_catalog_package_changed() {
        let entry = store_entry("acme");
        let plan = uninstall_plan(&entry, Some(&acme_spec()), Some(&record("npm", "someone-else-cli")), &tools());
        assert!(plan.steps.is_empty());
        assert!(plan.blocked.is_some());
    }

    #[test]
    fn a_script_install_has_no_removal_here() {
        let entry = store_entry("acme");
        let plan = uninstall_plan(&entry, Some(&acme_spec()), Some(&record("script", "")), &tools());
        assert!(plan.steps.is_empty());
        assert!(plan.blocked.is_some());
    }

    #[test]
    fn an_unknown_id_is_an_error_not_a_guess() {
        let err = plan("no_such_store_agent").unwrap_err();
        assert!(err.starts_with("INSTALL_UNKNOWN_AGENT"), "{err}");
    }
}

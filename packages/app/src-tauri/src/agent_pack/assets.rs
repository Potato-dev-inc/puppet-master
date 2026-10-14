//! The pack files, embedded from `integrations/skills` (the canonical copy; the plugin folders hold
//! byte-identical copies, checked by `tests::pack_files_match_canonical`).

/// `(skill name, SKILL.md)`. The installer writes `<skills dir>/<name>/SKILL.md`.
pub const SKILLS: &[(&str, &str)] = &[
    ("puppet-master", include_str!("../../../../../integrations/skills/puppet-master/SKILL.md")),
    ("pm-review", include_str!("../../../../../integrations/skills/pm-review/SKILL.md")),
    ("pm-delegate", include_str!("../../../../../integrations/skills/pm-delegate/SKILL.md")),
    ("pm-status", include_str!("../../../../../integrations/skills/pm-status/SKILL.md")),
];

/// `(relative path, text)` with LF line ends, whatever the checkout did.
pub fn files() -> Vec<(String, String)> {
    SKILLS
        .iter()
        .map(|(name, text)| (format!("{name}/SKILL.md"), text.replace("\r\n", "\n")))
        .collect()
}

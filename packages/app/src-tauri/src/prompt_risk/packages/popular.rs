//! Offline top lists of package names (SPEC-n33 section 3.3).
//!
//! The lists are text files next to this module (`popular/<eco>.txt`): one name per line, most used
//! first. Lines starting with `#` are comments. The files are compiled in and parsed once, on first
//! use. Names are stored in the form `Ecosystem::normalize` gives them.

use std::collections::HashMap;

use once_cell::sync::Lazy;

use super::model::Ecosystem;

struct Table {
    /// Normalized names, most used first.
    names: Vec<String>,
    /// Normalized name to its position in `names` (0 = most used).
    ranks: HashMap<String, u32>,
}

impl Table {
    fn parse(eco: Ecosystem, text: &str) -> Self {
        let mut names = Vec::new();
        let mut ranks = HashMap::new();
        for line in text.lines() {
            let name = line.trim();
            if name.is_empty() || name.starts_with('#') {
                continue;
            }
            let norm = eco.normalize(name);
            ranks.entry(norm.clone()).or_insert(names.len() as u32);
            names.push(norm);
        }
        Self { names, ranks }
    }
}

static NPM: Lazy<Table> = Lazy::new(|| Table::parse(Ecosystem::Npm, include_str!("popular/npm.txt")));
static PYPI: Lazy<Table> = Lazy::new(|| Table::parse(Ecosystem::Pypi, include_str!("popular/pypi.txt")));
static CRATES: Lazy<Table> =
    Lazy::new(|| Table::parse(Ecosystem::Crates, include_str!("popular/crates.txt")));
static NO_LIST: Lazy<Table> = Lazy::new(|| Table { names: Vec::new(), ranks: HashMap::new() });

fn table(eco: Ecosystem) -> &'static Table {
    match eco {
        Ecosystem::Npm => &NPM,
        Ecosystem::Pypi => &PYPI,
        Ecosystem::Crates => &CRATES,
        Ecosystem::Go | Ecosystem::Gems | Ecosystem::Winget | Ecosystem::Choco | Ecosystem::Scoop => &NO_LIST,
    }
}

/// Names in rank order (most used first). Empty for an ecosystem without a list.
pub fn list(eco: Ecosystem) -> &'static [String] {
    &table(eco).names
}

/// Position of a normalized name in the list, 0 = most used. `norm` is `Ecosystem::normalize` output.
pub fn rank(eco: Ecosystem, norm: &str) -> Option<u32> {
    table(eco).ranks.get(norm).copied()
}

/// Number of names in the list.
pub fn size(eco: Ecosystem) -> usize {
    table(eco).names.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Every list file, with its ecosystem.
    const FILES: [(Ecosystem, &str, &str); 3] = [
        (Ecosystem::Npm, "npm.txt", include_str!("popular/npm.txt")),
        (Ecosystem::Pypi, "pypi.txt", include_str!("popular/pypi.txt")),
        (Ecosystem::Crates, "crates.txt", include_str!("popular/crates.txt")),
    ];

    /// S14: every list has the header, every name passes `valid_name`, no duplicates, at most 2,000.
    #[test]
    fn every_list_file_has_header_valid_unique_names_and_fits_the_cap() {
        for (eco, file, text) in FILES {
            assert!(text.starts_with("# Package check top list"), "{file}: header line missing");
            let mut seen = HashSet::new();
            let mut count = 0usize;
            for line in text.lines() {
                let name = line.trim();
                if name.is_empty() || name.starts_with('#') {
                    continue;
                }
                count += 1;
                assert!(eco.valid_name(name), "{file}: {name} is not a valid name");
                assert!(seen.insert(eco.normalize(name)), "{file}: {name} is listed twice");
            }
            assert!(count <= 2000, "{file}: {count} names, the cap is 2,000");
            assert_eq!(size(eco), count, "{file}: parsed count differs from the file");
        }
    }

    #[test]
    fn real_lists_contain_well_known_names_in_rank_order() {
        assert_eq!(rank(Ecosystem::Npm, "lodash"), Some(0));
        assert!(rank(Ecosystem::Pypi, "requests").is_some());
        assert!(rank(Ecosystem::Crates, "serde").is_some());
        assert_eq!(rank(Ecosystem::Npm, "lodahs"), None);
        assert_eq!(rank(Ecosystem::Npm, "not-a-popular-name-zzz"), None);
    }

    #[test]
    fn ecosystems_without_a_list_are_empty() {
        for eco in [Ecosystem::Go, Ecosystem::Gems, Ecosystem::Winget, Ecosystem::Choco, Ecosystem::Scoop] {
            assert!(list(eco).is_empty(), "{eco:?} should have no list");
            assert_eq!(size(eco), 0);
            assert_eq!(rank(eco, "lodash"), None);
        }
    }

    #[test]
    fn list_and_rank_agree() {
        for eco in [Ecosystem::Npm, Ecosystem::Pypi, Ecosystem::Crates] {
            for (i, name) in list(eco).iter().enumerate() {
                assert_eq!(rank(eco, name), Some(i as u32), "{eco:?} {name}");
            }
        }
    }
}

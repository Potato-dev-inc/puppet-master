//! Which folders are too broad to be a companion's home.
//!
//! A coding agent reads instruction files (`AGENTS.md` and friends) from every PARENT of its working folder.
//! The companion brief lives in such a file inside the companion's home. If that home is the Desktop, or the
//! top of a drive, or a folder full of projects, then every agent working anywhere below it reads the brief
//! and answers as the companion. So a home must be a folder of its own.
//!
//! `broad_kind` is pure: it looks at the path text, and only reads the disk to look for `.git` folders below
//! the candidate. The "known folders" come from `Places`, so tests pass a fake home.

use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::files;
use super::model::{CompanionError, CompanionResult};

/// The error code of a refused home. The message says what to do instead.
pub const BROAD_CODE: &str = "COMPANION_FOLDER_BROAD";

/// How many folder levels below the candidate are searched for a `.git` folder (the candidate itself is level 0).
const PROJECT_DEPTH: usize = 3;
/// At most this many folders are opened in that search. A folder bigger than this is a broad one anyway.
const SCAN_BUDGET: usize = 2_000;
/// Folders that never hold a project of their own and can be huge.
const SKIP_DIRS: [&str; 2] = ["node_modules", "target"];

/// The folders of the person's account that every program looks in. The text is kept as it was given;
/// it is normalised (`norm`) only to compare.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Places {
    /// The user home (`USERPROFILE`).
    pub user_home: Option<String>,
    /// The roots of the cloud folders (`OneDrive`, `OneDriveConsumer`, `OneDriveCommercial`).
    pub cloud_roots: Vec<String>,
}

impl Places {
    /// Places for a known home folder (tests, and anything that already has the path).
    pub fn for_home(home: &Path) -> Self {
        Places { user_home: Some(home.to_string_lossy().to_string()), cloud_roots: Vec::new() }
    }

    /// The places of the running account, from the environment.
    pub fn from_env() -> Self {
        Self::from_home_and_env(files::user_home().as_deref())
    }

    /// `for_home`, plus the cloud roots the environment names.
    pub fn from_home_and_env(home: Option<&Path>) -> Self {
        let cloud_roots = ["OneDrive", "OneDriveConsumer", "OneDriveCommercial"]
            .iter()
            .filter_map(|name| std::env::var_os(name))
            .map(|v| v.to_string_lossy().to_string())
            .filter(|v| !v.trim().is_empty())
            .collect();
        Places { user_home: home.map(|h| h.to_string_lossy().to_string()), cloud_roots }
    }
}

/// Why a folder cannot be a companion's home.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Broad {
    /// The user home itself.
    UserHome,
    /// The top of a drive (`C:\`, `/`).
    DriveRoot,
    /// One of the standard folders, as a phrase: "your Desktop", "your Documents folder".
    Standard(String),
    /// A folder that has a `.git` folder in it, or in a folder up to three levels below.
    HasProjects,
}

impl Broad {
    /// Where the folder is, in a few words ("your Desktop"), for a sentence.
    pub fn place(&self) -> String {
        match self {
            Broad::UserHome => "your home folder".to_string(),
            Broad::DriveRoot => "the top of a drive".to_string(),
            Broad::Standard(phrase) => phrase.clone(),
            Broad::HasProjects => "a folder with other projects in it".to_string(),
        }
    }
}

/// What the desktop shows for a companion whose home is broad: where it is, and the folder to move to.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HomeProblem {
    /// "your Desktop", "a folder with other projects in it", ...
    pub place: String,
    /// The folder `Move to its own folder` would use.
    pub suggested_folder: String,
}

/// Lower-case, `\` only, no `\\?\` prefix and no trailing separator (except for a drive root like `c:\`).
pub fn norm(text: &str) -> String {
    let mut s = text.trim().replace('/', "\\");
    if let Some(rest) = s.strip_prefix("\\\\?\\") {
        s = rest.to_string();
    }
    let s = s.to_lowercase();
    let trimmed = s.trim_end_matches('\\');
    if trimmed.is_empty() {
        // "\" or "/" alone is the root.
        return if s.is_empty() { String::new() } else { "\\".to_string() };
    }
    if trimmed.len() == 2 && trimmed.ends_with(':') {
        return format!("{trimmed}\\");
    }
    trimmed.to_string()
}

fn is_drive_root(normed: &str) -> bool {
    normed == "\\" || (normed.len() == 3 && normed.ends_with(":\\") && normed.as_bytes()[0].is_ascii_alphabetic())
}

fn parent_and_name(normed: &str) -> Option<(&str, &str)> {
    let at = normed.rfind('\\')?;
    Some((&normed[..at], &normed[at + 1..]))
}

/// The standard folders under an account (or cloud) root, lower-case name and the phrase for a sentence.
const STANDARD: [(&str, &str); 6] = [
    ("desktop", "your Desktop"),
    ("documents", "your Documents folder"),
    ("downloads", "your Downloads folder"),
    ("pictures", "your Pictures folder"),
    ("music", "your Music folder"),
    ("videos", "your Videos folder"),
];

fn standard_name(name: &str) -> Option<&'static str> {
    STANDARD.iter().find(|(n, _)| *n == name).map(|(_, phrase)| *phrase)
}

/// The standard-folder verdict for a normalised path, or None.
fn standard_place(f: &str, places: &Places) -> Option<Broad> {
    let home_norm = places.user_home.as_deref().map(norm).filter(|h| !h.is_empty());
    let clouds: Vec<String> = places.cloud_roots.iter().map(|r| norm(r)).collect();
    let home = home_norm.as_deref();
    if let Some(h) = home {
        if f == h {
            return Some(Broad::UserHome);
        }
    }
    // A cloud root itself ("OneDrive", "OneDrive - Company").
    let cloud_root = |p: &str| -> bool {
        clouds.iter().any(|r| r == p)
            || home.is_some_and(|h| parent_and_name(p).is_some_and(|(parent, name)| parent == h && name.starts_with("onedrive")))
    };
    if cloud_root(f) {
        return Some(Broad::Standard("your OneDrive".to_string()));
    }
    let (parent, name) = parent_and_name(f)?;
    if let Some(phrase) = standard_name(name) {
        if home == Some(parent) || cloud_root(parent) {
            return Some(Broad::Standard(phrase.to_string()));
        }
    }
    // Puppet Master's own folders: the one that holds every companion home.
    if let Some(h) = home {
        let pm = format!("{h}\\puppetmaster");
        if f == pm || f == format!("{pm}\\companions") {
            return Some(Broad::Standard("the Puppet Master folder".to_string()));
        }
    }
    None
}

/// Why `folder` is too broad for a home, or None when it is fine. Looks at the disk below `folder` only to find projects.
pub fn broad_kind(folder: &Path, places: &Places) -> Option<Broad> {
    let f = norm(&folder.to_string_lossy());
    if is_drive_root(&f) {
        return Some(Broad::DriveRoot);
    }
    if let Some(found) = standard_place(&f, places) {
        return Some(found);
    }
    has_projects(folder).then_some(Broad::HasProjects)
}

/// True when `dir` has a `.git` entry, or any folder up to `PROJECT_DEPTH` levels below it has one.
/// Links are not followed. A folder with too many subfolders to look through counts as broad.
pub fn has_projects(dir: &Path) -> bool {
    let mut budget = SCAN_BUDGET;
    walk(dir, 0, &mut budget)
}

fn walk(dir: &Path, depth: usize, budget: &mut usize) -> bool {
    let Ok(entries) = fs::read_dir(dir) else { return false };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == ".git" {
            return true;
        }
        let Ok(kind) = entry.file_type() else { continue };
        if !kind.is_dir() || kind.is_symlink() || depth >= PROJECT_DEPTH {
            continue;
        }
        if SKIP_DIRS.iter().any(|skip| name.eq_ignore_ascii_case(skip)) {
            continue;
        }
        if *budget == 0 {
            return true;
        }
        *budget -= 1;
        if walk(&entry.path(), depth + 1, budget) {
            return true;
        }
    }
    false
}

/// The folder the person is pointed to instead: the default home of this companion, else a folder of its own inside `folder`.
pub fn suggested_home(folder: &Path, id: &str, places: &Places) -> PathBuf {
    match &places.user_home {
        Some(home) => files::default_folder(Path::new(home), id),
        None => folder.join(id),
    }
}

/// The refusal sentence: simple English, and the fix.
pub fn refusal_message(broad: &Broad, name: &str, suggested: &Path) -> String {
    let what = match broad {
        Broad::HasProjects => "That folder".to_string(),
        other => {
            let place = other.place();
            let mut chars = place.chars();
            chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
        }
    };
    format!(
        "Pick a folder of its own. {what} has other projects in it, and agents there would read {name}'s notes. Try {}.",
        suggested.display()
    )
}

/// Refuses a home that is too broad (`COMPANION_FOLDER_BROAD`). `Ok` for a folder of its own.
pub fn check_home(folder: &Path, name: &str, id: &str, places: &Places) -> CompanionResult<()> {
    match broad_kind(folder, places) {
        None => Ok(()),
        Some(broad) => Err(CompanionError::new(BROAD_CODE, refusal_message(&broad, name, &suggested_home(folder, id, places)))),
    }
}

/// The problem to show for a companion whose saved home is broad. None when the home is fine.
pub fn problem_of(folder: &Path, id: &str, places: &Places) -> Option<HomeProblem> {
    let broad = broad_kind(folder, places)?;
    Some(HomeProblem { place: broad.place(), suggested_folder: suggested_home(folder, id, places).display().to_string() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(prefix: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("{prefix}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn places() -> Places {
        Places::for_home(Path::new("C:\\Users\\you"))
    }

    fn kind(path: &str) -> Option<Broad> {
        broad_kind(Path::new(path), &places())
    }

    #[test]
    fn the_user_home_is_too_broad() {
        assert_eq!(kind("C:\\Users\\you"), Some(Broad::UserHome));
        assert_eq!(kind("c:/users/YOU/"), Some(Broad::UserHome));
        assert_eq!(kind("\\\\?\\C:\\Users\\you"), Some(Broad::UserHome));
    }

    #[test]
    fn a_drive_root_is_too_broad() {
        assert_eq!(kind("C:\\"), Some(Broad::DriveRoot));
        assert_eq!(kind("D:"), Some(Broad::DriveRoot));
        assert_eq!(kind("d:/"), Some(Broad::DriveRoot));
    }

    #[test]
    fn the_standard_folders_are_too_broad() {
        assert_eq!(kind("C:\\Users\\you\\Desktop"), Some(Broad::Standard("your Desktop".into())));
        assert_eq!(kind("C:\\Users\\you\\desktop\\"), Some(Broad::Standard("your Desktop".into())));
        assert_eq!(kind("C:\\Users\\you\\Documents"), Some(Broad::Standard("your Documents folder".into())));
        assert_eq!(kind("C:\\Users\\you\\Downloads"), Some(Broad::Standard("your Downloads folder".into())));
        assert_eq!(kind("C:\\Users\\you\\OneDrive"), Some(Broad::Standard("your OneDrive".into())));
        assert_eq!(kind("C:\\Users\\you\\OneDrive - Company"), Some(Broad::Standard("your OneDrive".into())));
        assert_eq!(kind("C:\\Users\\you\\OneDrive\\Desktop"), Some(Broad::Standard("your Desktop".into())));
        assert_eq!(kind("C:\\Users\\you\\OneDrive - Company\\Documents"), Some(Broad::Standard("your Documents folder".into())));
        assert_eq!(kind("C:\\Users\\you\\PuppetMaster\\companions"), Some(Broad::Standard("the Puppet Master folder".into())));
    }

    #[test]
    fn a_cloud_root_from_the_environment_counts_too() {
        let mut p = places();
        p.cloud_roots.push(norm("E:\\Cloud\\Sync"));
        assert_eq!(broad_kind(Path::new("E:\\Cloud\\Sync"), &p), Some(Broad::Standard("your OneDrive".into())));
        assert_eq!(broad_kind(Path::new("E:\\Cloud\\Sync\\Desktop"), &p), Some(Broad::Standard("your Desktop".into())));
        assert_eq!(broad_kind(Path::new("E:\\Cloud\\Sync\\pipi"), &p), None);
    }

    #[test]
    fn a_folder_of_its_own_is_fine() {
        // None of these exist, so there is nothing to find in them.
        assert_eq!(kind("C:\\Users\\you\\Desktop\\pipi"), None);
        assert_eq!(kind("C:\\Users\\you\\Documents\\companions\\pipi"), None);
        assert_eq!(kind("D:\\companions\\pipi"), None);
    }

    #[test]
    fn the_default_folder_is_fine() {
        let default = files::default_folder(Path::new("C:\\Users\\you"), "pipi");
        assert_eq!(broad_kind(&default, &places()), None);
        assert!(check_home(&default, "Pipi", "pipi", &places()).is_ok());
    }

    #[test]
    fn a_real_empty_folder_is_fine() {
        let dir = temp("pm-broad-empty");
        assert_eq!(broad_kind(&dir, &Places::for_home(Path::new("C:\\Users\\you"))), None);
    }

    #[test]
    fn a_folder_with_a_git_folder_in_it_or_below_it_is_broad() {
        let here = temp("pm-broad-git-here");
        fs::create_dir_all(here.join(".git")).unwrap();
        assert_eq!(broad_kind(&here, &places()), Some(Broad::HasProjects));

        // The Desktop case: projects a few levels down.
        let desk = temp("pm-broad-git-nested");
        fs::create_dir_all(desk.join("work2").join("shop").join(".git")).unwrap();
        assert_eq!(broad_kind(&desk, &places()), Some(Broad::HasProjects));

        // A git file (a worktree) counts as well.
        let wt = temp("pm-broad-git-file");
        fs::create_dir_all(wt.join("a")).unwrap();
        fs::write(wt.join("a").join(".git"), "gitdir: x").unwrap();
        assert_eq!(broad_kind(&wt, &places()), Some(Broad::HasProjects));
    }

    #[test]
    fn projects_deeper_than_three_levels_or_inside_node_modules_or_target_are_not_looked_for() {
        let deep = temp("pm-broad-git-deep");
        fs::create_dir_all(deep.join("a").join("b").join("c").join("d").join(".git")).unwrap();
        assert_eq!(broad_kind(&deep, &places()), None);
        let edge = temp("pm-broad-git-edge");
        fs::create_dir_all(edge.join("a").join("b").join("c").join(".git")).unwrap();
        assert_eq!(broad_kind(&edge, &places()), Some(Broad::HasProjects));

        let skipped = temp("pm-broad-git-skipped");
        fs::create_dir_all(skipped.join("node_modules").join("pkg").join(".git")).unwrap();
        fs::create_dir_all(skipped.join("target").join("x").join(".git")).unwrap();
        assert_eq!(broad_kind(&skipped, &places()), None);
    }

    #[test]
    fn the_refusal_is_plain_and_offers_the_fix() {
        let desk = Path::new("C:\\Users\\you\\Desktop");
        let err = check_home(desk, "Pipi", "pipi", &places()).unwrap_err();
        assert_eq!(err.code, BROAD_CODE);
        assert!(err.message.starts_with("Pick a folder of its own. Your Desktop has other projects in it,"), "{}", err.message);
        assert!(err.message.contains("agents there would read Pipi's notes."));
        let suggested = files::default_folder(Path::new("C:\\Users\\you"), "pipi");
        assert!(err.message.contains(&suggested.display().to_string()), "{}", err.message);

        let here = temp("pm-broad-msg");
        fs::create_dir_all(here.join(".git")).unwrap();
        let err = check_home(&here, "Pipi", "pipi", &places()).unwrap_err();
        assert!(err.message.starts_with("Pick a folder of its own. That folder has other projects in it,"), "{}", err.message);
        let err = check_home(Path::new("C:\\"), "Pipi", "pipi", &places()).unwrap_err();
        assert!(err.message.contains("The top of a drive has other projects in it"));
    }

    #[test]
    fn without_a_known_home_the_suggestion_is_a_folder_inside() {
        let none = Places::default();
        let err = check_home(Path::new("D:\\"), "Pipi", "pipi", &none).unwrap_err();
        assert!(err.message.contains("pipi"), "{}", err.message);
        assert_eq!(suggested_home(Path::new("D:\\x"), "pipi", &none), Path::new("D:\\x").join("pipi"));
    }

    #[test]
    fn the_problem_names_the_place_and_the_new_home() {
        let problem = problem_of(Path::new("C:\\Users\\you\\Desktop"), "pipi", &places()).unwrap();
        assert_eq!(problem.place, "your Desktop");
        assert_eq!(problem.suggested_folder, files::default_folder(Path::new("C:\\Users\\you"), "pipi").display().to_string());
        assert!(problem_of(&files::default_folder(Path::new("C:\\Users\\you"), "pipi"), "pipi", &places()).is_none());
    }
}

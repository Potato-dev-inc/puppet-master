//! Collision Alert: finds the git repo of a pane's folder, with no process. The only reads are a
//! few `stat` calls and two small text files (a worktree pointer and its `commondir`).

use std::path::{Path, PathBuf};

use super::model::RepoFact;

/// How many parents above the pane's folder are checked for a `.git` marker.
const MAX_PARENTS: usize = 16;
/// A worktree pointer file or its `commondir` file is never bigger than this.
const MAX_POINTER_BYTES: u64 = 4 * 1024;

/// The git repo that `cwd` (a pane's folder) belongs to, if any.
///
/// Walks up at most 16 parents to the first `.git`. A `.git` folder is a main checkout: that folder
/// is the common dir and its parent the root. A `.git` file is a linked worktree copy: it holds
/// `gitdir: <path>`, and the gitdir's `commondir` file names the common dir. A broken, oversized or
/// bare target gives `None`. Never starts a process.
pub fn repo_of(cwd: &Path) -> Option<RepoFact> {
    if cwd.as_os_str().is_empty() {
        return None;
    }
    for dir in cwd.ancestors().take(MAX_PARENTS + 1) {
        let marker = dir.join(".git");
        let Ok(meta) = std::fs::symlink_metadata(&marker) else {
            continue;
        };
        // A linked `.git` could point anywhere: never follow it.
        if meta.file_type().is_symlink() {
            return None;
        }
        if meta.is_dir() {
            return Some(RepoFact {
                key: crate::project_path::comparable_project_path(&marker),
                root: dir.to_string_lossy().into_owned(),
                linked: false,
            });
        }
        if meta.is_file() {
            return linked(&marker, dir);
        }
        return None;
    }
    None
}

/// A worktree copy: the pointer file in `dir` names a gitdir, whose `commondir` names the repo.
fn linked(pointer: &Path, dir: &Path) -> Option<RepoFact> {
    let text = read_small(pointer)?;
    let target = text.lines().next()?.strip_prefix("gitdir:")?.trim();
    if target.is_empty() {
        return None;
    }
    let gitdir = resolve(dir, target);
    if !gitdir.is_dir() {
        return None;
    }
    // A gitdir without `commondir` is a bare repo or a submodule: not watched in v1.
    let common_text = read_small(&gitdir.join("commondir"))?;
    let common_line = common_text.lines().next()?.trim();
    if common_line.is_empty() {
        return None;
    }
    let common = resolve(&gitdir, common_line);
    if !common.is_dir() {
        return None;
    }
    Some(RepoFact {
        key: crate::project_path::comparable_project_path(&common),
        root: dir.to_string_lossy().into_owned(),
        linked: true,
    })
}

/// `value` as a path: absolute as it is, relative to `base` otherwise.
fn resolve(base: &Path, value: &str) -> PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

/// The text of a small plain file (not a link, at most 4 KiB, no NUL byte), or `None`.
fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_POINTER_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    /// A temp folder that is removed on drop (git objects are read-only on Windows).
    struct Tmp(PathBuf);

    impl Tmp {
        fn new(tag: &str) -> Tmp {
            let dir = std::env::temp_dir().join(format!(
                "pmcol-repo-{tag}-{}",
                &uuid::Uuid::new_v4().to_string()[..8]
            ));
            std::fs::create_dir_all(&dir).unwrap();
            Tmp(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Tmp {
        fn drop(&mut self) {
            clear_readonly(&self.0);
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn clear_readonly(path: &Path) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if let Ok(meta) = std::fs::symlink_metadata(&p) {
                    if meta.is_dir() && !meta.file_type().is_symlink() {
                        clear_readonly(&p);
                    } else if let Ok(mut perm) = std::fs::metadata(&p).map(|m| m.permissions()) {
                        if perm.readonly() {
                            perm.set_readonly(false);
                            let _ = std::fs::set_permissions(&p, perm);
                        }
                    }
                }
            }
        }
    }

    fn git_available() -> bool {
        Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
            .expect("git runs");
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// A repo on `main` with one commit, in a fresh temp folder.
    fn repo(tag: &str) -> Tmp {
        let tmp = Tmp::new(tag);
        git(tmp.path(), &["init", "-q", "-b", "main"]);
        git(tmp.path(), &["config", "user.name", "Test"]);
        git(tmp.path(), &["config", "user.email", "test@example.com"]);
        git(tmp.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(tmp.path().join("a.txt"), "alpha\n").unwrap();
        git(tmp.path(), &["add", "-A"]);
        git(tmp.path(), &["commit", "-q", "-m", "initial"]);
        tmp
    }

    fn key_of(path: &Path) -> String {
        crate::project_path::comparable_project_path(path)
    }

    #[test]
    fn main_checkout_is_its_own_repo() {
        if !git_available() {
            return;
        }
        let tmp = repo("main");
        let fact = repo_of(tmp.path()).expect("a repo");
        assert_eq!(fact.key, key_of(&tmp.path().join(".git")));
        assert_eq!(fact.root, tmp.path().to_string_lossy());
        assert!(!fact.linked);
    }

    #[test]
    fn subfolder_cwd_finds_the_same_repo() {
        if !git_available() {
            return;
        }
        let tmp = repo("sub");
        let sub = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&sub).unwrap();
        let from_root = repo_of(tmp.path()).unwrap();
        let from_sub = repo_of(&sub).unwrap();
        assert_eq!(from_sub, from_root);
    }

    #[test]
    fn worktree_copy_maps_to_the_same_key_as_its_origin() {
        if !git_available() {
            return;
        }
        let origin = repo("origin");
        // A sibling folder of the origin, named without `..` so the root string compares exactly.
        let copy_dir = origin.path().parent().unwrap().join(format!(
            "pmcol-repo-copy-{}",
            &uuid::Uuid::new_v4().to_string()[..8]
        ));
        let copy = Tmp(copy_dir.clone());
        git(origin.path(), &["worktree", "add", "-q", "--detach", &copy_dir.to_string_lossy()]);
        let origin_fact = repo_of(origin.path()).unwrap();
        let copy_fact = repo_of(&copy_dir).unwrap();
        assert_eq!(copy_fact.key, origin_fact.key);
        assert_eq!(copy_fact.root, copy_dir.to_string_lossy());
        assert!(copy_fact.linked);
        assert!(!origin_fact.linked);
        drop(copy);
    }

    #[test]
    fn copy_under_puppet_master_folder_is_found() {
        if !git_available() {
            return;
        }
        let origin = repo("pm");
        let copy_dir = origin.path().join(".puppet-master").join("worktrees").join("x");
        std::fs::create_dir_all(copy_dir.parent().unwrap()).unwrap();
        git(origin.path(), &["worktree", "add", "-q", "--detach", &copy_dir.to_string_lossy()]);
        let copy_fact = repo_of(&copy_dir).expect("the copy is a repo");
        assert_eq!(copy_fact.key, repo_of(origin.path()).unwrap().key);
        assert_eq!(copy_fact.root, copy_dir.to_string_lossy());
        assert!(copy_fact.linked);
    }

    #[test]
    fn folder_without_git_is_no_repo() {
        let tmp = Tmp::new("nogit");
        let deep = tmp.path().join("a").join("b");
        std::fs::create_dir_all(&deep).unwrap();
        // No `.git` in this folder or its parents (the temp folder is not inside a repo).
        assert!(repo_of(&deep).is_none());
        assert!(repo_of(Path::new("")).is_none());
    }

    #[test]
    fn broken_pointer_is_no_repo() {
        let tmp = Tmp::new("broken");
        let folder = tmp.path().join("copy");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(".git"), "gitdir: Z:/does/not/exist/worktrees/x\n").unwrap();
        assert!(repo_of(&folder).is_none());
        // A pointer that names no gitdir line at all.
        std::fs::write(folder.join(".git"), "hello\n").unwrap();
        assert!(repo_of(&folder).is_none());
    }

    #[test]
    fn gitdir_without_commondir_is_no_repo() {
        // A bare or submodule-style gitdir: it has no `commondir`, so it is not watched in v1.
        let tmp = Tmp::new("bare");
        let gitdir = tmp.path().join("modules").join("x");
        std::fs::create_dir_all(&gitdir).unwrap();
        let folder = tmp.path().join("copy");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        assert!(repo_of(&folder).is_none());
    }

    #[test]
    fn huge_pointer_file_is_refused() {
        let tmp = Tmp::new("huge");
        let gitdir = tmp.path().join("gd");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("commondir"), "..\n").unwrap();
        let folder = tmp.path().join("copy");
        std::fs::create_dir_all(&folder).unwrap();
        let mut text = format!("gitdir: {}\n", gitdir.display());
        text.push_str(&" ".repeat(5_000));
        std::fs::write(folder.join(".git"), &text).unwrap();
        assert!(repo_of(&folder).is_none());
        // The same pointer, small enough, is accepted.
        std::fs::write(folder.join(".git"), format!("gitdir: {}\n", gitdir.display())).unwrap();
        assert!(repo_of(&folder).is_some());
    }

    #[test]
    fn pointer_with_nul_byte_is_refused() {
        let tmp = Tmp::new("nul");
        let gitdir = tmp.path().join("gd");
        std::fs::create_dir_all(&gitdir).unwrap();
        std::fs::write(gitdir.join("commondir"), "..\n").unwrap();
        let folder = tmp.path().join("copy");
        std::fs::create_dir_all(&folder).unwrap();
        let mut bytes = format!("gitdir: {}\n", gitdir.display()).into_bytes();
        bytes.push(0);
        std::fs::write(folder.join(".git"), bytes).unwrap();
        assert!(repo_of(&folder).is_none());
    }

    #[test]
    fn sixteen_parents_are_checked_and_no_more() {
        let tmp = Tmp::new("depth");
        let mut deep = tmp.path().to_path_buf();
        for i in 0..17 {
            deep = deep.join(format!("d{i}"));
        }
        std::fs::create_dir_all(&deep).unwrap();
        // A `.git` 17 parents up is beyond the walk: no repo found from the bottom folder.
        std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
        assert!(repo_of(&deep).is_none());
        // A `.git` 16 parents up is inside the walk.
        let mut mid = tmp.path().to_path_buf();
        for i in 0..15 {
            mid = mid.join(format!("d{i}"));
        }
        let fact = repo_of(&mid.join("d15")).expect("16 parents up is checked");
        assert_eq!(fact.root, tmp.path().to_string_lossy());
    }
}

//! Detects a project that uses its own package server, so no package name is sent to a public
//! registry (SPEC-n33 section 3.9, S3). Pure: file reads and env reads are injected by the caller,
//! so tests never touch the real home folder. Only the keys named in the table are looked at. Token
//! lines are skipped before their value is read. Nothing is stored, returned or logged.

use std::path::{Path, PathBuf};

use super::model::Ecosystem;

/// Most text read from one config file (SPEC-n33 section 3.9).
const MAX_CONFIG_BYTES: usize = 64 * 1024;
const NPM_DEFAULT_HOST: &str = "registry.npmjs.org";
const PYPI_DEFAULT_HOST: &str = "pypi.org";
const GEMS_DEFAULT_HOST: &str = "rubygems.org";
const GO_DEFAULT_PROXY: &str = "proxy.golang.org";
const PYPI_ENV: [&str; 7] = [
    "PIP_INDEX_URL",
    "PIP_EXTRA_INDEX_URL",
    "UV_INDEX_URL",
    "UV_DEFAULT_INDEX",
    "UV_INDEX",
    "UV_EXTRA_INDEX_URL",
    "PIP_FIND_LINKS",
];
/// Table headers that add another package index.
const INDEX_TABLES: [&str; 4] = [
    "[[index]]",
    "[[tool.uv.index]]",
    "[[tool.poetry.source]]",
    "[[tool.pdm.source]]",
];

/// What `uses_own_server` may look at: the project folder, the home folder, and two injected readers.
pub struct Look<'a> {
    pub workspace: Option<&'a Path>,
    pub home: Option<&'a Path>,
    /// Text of one file, or None when it is missing. The caller should cap it at 64 KiB;
    /// `read_capped` cuts it again, so a longer answer is safe too.
    pub read: &'a dyn Fn(&Path) -> Option<String>,
    pub env: &'a dyn Fn(&str) -> Option<String>,
}

/// True when packages named `name` of `eco` come from a server other than the public registry, so
/// a lookup would send an internal name to a public place. Missing or unreadable config means false.
pub fn uses_own_server(eco: Ecosystem, name: &str, look: &Look<'_>) -> bool {
    match eco {
        Ecosystem::Npm => npm(name, look),
        Ecosystem::Pypi => pypi(look),
        Ecosystem::Crates => crates(look),
        Ecosystem::Go => go(name, look),
        Ecosystem::Gems => gems(look),
        Ecosystem::Winget | Ecosystem::Choco | Ecosystem::Scoop => false,
    }
}

fn npm(name: &str, look: &Look<'_>) -> bool {
    let scope = npm_scope(name);
    // The environment decides first, so no file is opened when it already answers.
    let env_elsewhere = ["NPM_CONFIG_REGISTRY", "npm_config_registry"]
        .iter()
        .filter_map(|key| env_value(look, key))
        .any(|url| !is_default_host(&url, NPM_DEFAULT_HOST));
    if env_elsewhere {
        return true;
    }
    let npmrc = [look.workspace, look.home]
        .into_iter()
        .flatten()
        .map(|dir| dir.join(".npmrc"));
    for path in npmrc {
        if read_capped(look, &path).is_some_and(|text| npmrc_elsewhere(&text, scope)) {
            return true;
        }
    }
    look.workspace
        .map(|dir| dir.join(".yarnrc.yml"))
        .and_then(|path| read_capped(look, &path))
        .is_some_and(|text| yarnrc_elsewhere(&text, scope))
}

/// `@acme` for `@acme/tool`; None for a name without a scope.
fn npm_scope(name: &str) -> Option<&str> {
    if !name.starts_with('@') {
        return None;
    }
    name.split_once('/').map(|(scope, _)| scope)
}

/// `registry=<url>`, or `@<scope>:registry=<url>` for the name's scope, with a host other than the public one.
fn npmrc_elsewhere(text: &str, scope: Option<&str>) -> bool {
    text.lines().filter_map(npmrc_pair).any(|(key, value)| {
        if key.eq_ignore_ascii_case("registry") {
            return !is_default_host(value, NPM_DEFAULT_HOST);
        }
        let lower = key.to_ascii_lowercase();
        match (scope, lower.strip_suffix(":registry")) {
            (Some(s), Some(k)) => {
                k.eq_ignore_ascii_case(s) && !is_default_host(value, NPM_DEFAULT_HOST)
            }
            _ => false,
        }
    })
}

/// Key and value of one .npmrc line. Comments and token lines give None: a key that starts with
/// `//` (`//host/:_authToken=`) or contains `_auth` is skipped before its value is looked at.
fn npmrc_pair(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
        return None;
    }
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.starts_with("//") || key.to_ascii_lowercase().contains("_auth") {
        return None;
    }
    Some((key, value))
}

/// `npmRegistryServer:` with a host other than the public one, or a `npmScopes:` entry for the name's scope.
fn yarnrc_elsewhere(text: &str, scope: Option<&str>) -> bool {
    let bare = scope.map(|s| s.trim_start_matches('@'));
    // Indent of the `npmScopes:` line while its block lasts.
    let mut in_scopes: Option<usize> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if in_scopes.is_some_and(|head| indent <= head) {
            in_scopes = None;
        }
        if trimmed == "npmScopes:" {
            in_scopes = Some(indent);
            continue;
        }
        if in_scopes.is_some() {
            let key = trimmed
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(|c| c == '"' || c == '\'');
            if bare.is_some_and(|s| key.eq_ignore_ascii_case(s)) {
                return true;
            }
        }
        if let Some(value) = trimmed.strip_prefix("npmRegistryServer:") {
            if !is_default_host(value, NPM_DEFAULT_HOST) {
                return true;
            }
        }
    }
    false
}

fn pypi(look: &Look<'_>) -> bool {
    let env_elsewhere = PYPI_ENV
        .iter()
        .filter_map(|key| env_value(look, key))
        .any(|value| {
            value
                .split(|c: char| c.is_whitespace() || c == ',')
                .filter(|token| !token.is_empty())
                .any(pypi_token_elsewhere)
        });
    if env_elsewhere {
        return true;
    }
    let mut files: Vec<PathBuf> = Vec::new();
    if let Some(dir) = look.workspace {
        files.push(dir.join("pip.conf"));
        files.push(dir.join("uv.toml"));
        files.push(dir.join("pyproject.toml"));
    }
    if let Some(dir) = look.home {
        files.push(dir.join(".config").join("pip").join("pip.conf"));
        files.push(dir.join(".pip").join("pip.conf"));
    }
    if let Some(appdata) = env_value(look, "APPDATA") {
        files.push(Path::new(&appdata).join("pip").join("pip.ini"));
    }
    files
        .iter()
        .any(|path| read_capped(look, path).is_some_and(|text| pip_text_elsewhere(&text)))
}

/// One index value from the environment. uv's `name=url` form names the index after the `=`.
fn pypi_token_elsewhere(token: &str) -> bool {
    let url = match token.split_once('=') {
        Some((name, url)) if !name.contains("://") && url.contains("://") => url,
        _ => token,
    };
    !is_default_host(url, PYPI_DEFAULT_HOST)
}

/// pip.conf, pip.ini, uv.toml or pyproject.toml: an index URL other than pypi.org, or a table that adds an index.
fn pip_text_elsewhere(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if INDEX_TABLES.contains(&line) {
            return true;
        }
        if line.is_empty()
            || line.starts_with('#')
            || line.starts_with(';')
            || line.starts_with('[')
        {
            return false;
        }
        let Some(cut) = line.find(['=', ':']) else {
            return false;
        };
        let key = line[..cut].trim();
        let value = line[cut + 1..].trim();
        (key.eq_ignore_ascii_case("index-url") || key.eq_ignore_ascii_case("extra-index-url"))
            && !value.is_empty()
            && !is_default_host(value, PYPI_DEFAULT_HOST)
    })
}

fn crates(look: &Look<'_>) -> bool {
    let Some(dir) = look.workspace else {
        return false;
    };
    [
        dir.join(".cargo").join("config.toml"),
        dir.join(".cargo").join("config"),
    ]
    .iter()
    .any(|path| read_capped(look, path).is_some_and(|text| cargo_text_elsewhere(&text)))
}

/// A non-empty `replace-with` under `[source.crates-io]`.
fn cargo_text_elsewhere(text: &str) -> bool {
    let mut in_table = false;
    for line in text.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[') {
            let header = header.split('#').next().unwrap_or("");
            let name: String = header
                .chars()
                .filter(|c| !matches!(c, ']' | '"' | '\'' | ' ' | '\t'))
                .collect();
            in_table = name == "source.crates-io";
        } else if in_table {
            if let Some((key, value)) = line.split_once('=') {
                let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
                if key.trim() == "replace-with" && !value.is_empty() {
                    return true;
                }
            }
        }
    }
    false
}

fn go(name: &str, look: &Look<'_>) -> bool {
    // A GOPROXY that does not list the public proxy (`off`, `direct`, a company proxy).
    if let Some(proxy) = env_value(look, "GOPROXY") {
        let lists_public = proxy
            .split([',', '|'])
            .any(|entry| is_default_host(entry, GO_DEFAULT_PROXY));
        if !lists_public {
            return true;
        }
    }
    ["GOPRIVATE", "GONOPROXY"]
        .iter()
        .filter_map(|key| env_value(look, key))
        .any(|globs| go_globs_match(&globs, name))
}

/// True when any comma-separated pattern in `globs` matches the module path `module`.
fn go_globs_match(globs: &str, module: &str) -> bool {
    globs
        .split(',')
        .map(str::trim)
        .filter(|glob| !glob.is_empty())
        .any(|glob| go_glob_matches(glob, module))
}

/// Go's prefix rule: a glob with N slashes is matched against the first N+1 elements of the module
/// path (`github.com/acme` matches `github.com/acme/tool`; `*.corp.test` matches `git.corp.test/x`).
fn go_glob_matches(glob: &str, module: &str) -> bool {
    let mut slashes = glob.matches('/').count();
    let mut prefix_end = module.len();
    for (i, b) in module.bytes().enumerate() {
        if b == b'/' {
            if slashes == 0 {
                prefix_end = i;
                break;
            }
            slashes -= 1;
        }
    }
    slashes == 0 && path_match(glob, &module[..prefix_end])
}

/// `path.Match` for the forms GOPRIVATE uses: `*`, `?`, `[...]` and `\` escapes. `*` and `?` never cross `/`.
fn path_match(pattern: &str, name: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = name.chars().collect();
    match_chars(&p, &s)
}

fn match_chars(p: &[char], s: &[char]) -> bool {
    let Some((&c, rest)) = p.split_first() else {
        return s.is_empty();
    };
    match c {
        '*' => (0..=s.len())
            .take_while(|&i| i == 0 || s[i - 1] != '/')
            .any(|i| match_chars(rest, &s[i..])),
        '?' => !s.is_empty() && s[0] != '/' && match_chars(rest, &s[1..]),
        '\\' => match rest.split_first() {
            Some((&e, rest2)) => !s.is_empty() && s[0] == e && match_chars(rest2, &s[1..]),
            None => false,
        },
        '[' => match_class(rest, s),
        _ => !s.is_empty() && s[0] == c && match_chars(rest, &s[1..]),
    }
}

/// A `[...]` class: `rest` starts after the `[`. Returns false for a class with no closing `]`.
fn match_class(p: &[char], s: &[char]) -> bool {
    let Some(&c) = s.first() else {
        return false;
    };
    if c == '/' {
        return false;
    }
    let (negate, mut i) = if p.first() == Some(&'^') {
        (true, 1)
    } else {
        (false, 0)
    };
    let mut matched = false;
    let mut first = true;
    loop {
        match p.get(i) {
            None => return false,
            Some(&']') if !first => break,
            Some(&lo) => {
                let (lo, next) = if lo == '\\' && i + 1 < p.len() {
                    (p[i + 1], i + 2)
                } else {
                    (lo, i + 1)
                };
                let (hi, next) =
                    if p.get(next) == Some(&'-') && p.get(next + 1).is_some_and(|&h| h != ']') {
                        (p[next + 1], next + 2)
                    } else {
                        (lo, next)
                    };
                if lo <= c && c <= hi {
                    matched = true;
                }
                i = next;
            }
        }
        first = false;
    }
    if matched == negate {
        return false;
    }
    match_chars(&p[i + 1..], &s[1..])
}

fn gems(look: &Look<'_>) -> bool {
    let gemfile = look.workspace.map(|dir| dir.join("Gemfile"));
    if gemfile
        .and_then(|path| read_capped(look, &path))
        .is_some_and(|text| gemfile_elsewhere(&text))
    {
        return true;
    }
    look.home
        .map(|dir| dir.join(".gemrc"))
        .and_then(|path| read_capped(look, &path))
        .is_some_and(|text| gemrc_elsewhere(&text))
}

/// `source "<url>"` (either quote) in a Gemfile, with a host other than rubygems.org.
fn gemfile_elsewhere(text: &str) -> bool {
    text.lines().any(|line| {
        let Some(rest) = line.trim().strip_prefix("source") else {
            return false;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            return false;
        };
        let url = rest[1..].split(quote).next().unwrap_or("");
        !url.is_empty() && !is_default_host(url, GEMS_DEFAULT_HOST)
    })
}

/// `:sources:` list in .gemrc with an entry whose host is not rubygems.org.
fn gemrc_elsewhere(text: &str) -> bool {
    let mut in_sources = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == ":sources:" {
            in_sources = true;
            continue;
        }
        if !in_sources {
            continue;
        }
        if let Some(item) = trimmed.strip_prefix('-') {
            if !item.trim().is_empty() && !is_default_host(item, GEMS_DEFAULT_HOST) {
                return true;
            }
        } else if !trimmed.is_empty() {
            in_sources = false;
        }
    }
    false
}

/// The environment value, trimmed; an empty value counts as not set.
fn env_value(look: &Look<'_>, key: &str) -> Option<String> {
    (look.env)(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The file's text, cut to 64 KiB at a character boundary.
fn read_capped(look: &Look<'_>, path: &Path) -> Option<String> {
    let mut text = (look.read)(path)?;
    if text.len() > MAX_CONFIG_BYTES {
        let mut cut = MAX_CONFIG_BYTES;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
    }
    Some(text)
}

fn is_default_host(value: &str, default: &str) -> bool {
    host_of(value).is_some_and(|host| host == default)
}

/// The host of a URL or bare address: lowercase, no scheme, userinfo, port or path. None when empty.
fn host_of(value: &str) -> Option<String> {
    let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
    let after_scheme = value.find("://").map_or(value, |i| &value[i + 3..]);
    let authority = after_scheme.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next()?;
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next()?
    } else {
        authority.split(':').next()?
    };
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    (!host.is_empty()).then_some(host)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    /// Fake files and environment. `opened` records every path the code asked to read.
    #[derive(Default)]
    struct Fake {
        files: BTreeMap<PathBuf, String>,
        env: BTreeMap<String, String>,
        opened: RefCell<Vec<PathBuf>>,
    }

    impl Fake {
        fn with_file(mut self, path: PathBuf, text: &str) -> Self {
            self.files.insert(path, text.to_string());
            self
        }

        fn with_env(mut self, key: &str, value: &str) -> Self {
            self.env.insert(key.to_string(), value.to_string());
            self
        }

        fn opened(&self) -> Vec<PathBuf> {
            self.opened.borrow().clone()
        }

        fn ask(&self, eco: Ecosystem, name: &str) -> bool {
            let ws = Path::new("ws");
            let home = Path::new("home");
            let read = |path: &Path| {
                self.opened.borrow_mut().push(path.to_path_buf());
                self.files.get(path).cloned()
            };
            let env = |key: &str| self.env.get(key).cloned();
            let look = Look {
                workspace: Some(ws),
                home: Some(home),
                read: &read,
                env: &env,
            };
            uses_own_server(eco, name, &look)
        }
    }

    /// `root` joined with `parts`, for fake file paths.
    fn at(root: &str, parts: &[&str]) -> PathBuf {
        parts
            .iter()
            .fold(PathBuf::from(root), |path, part| path.join(part))
    }

    #[test]
    fn npm_scope_line_matches_only_its_scope() {
        let fake = Fake::default().with_file(
            at("ws", &[".npmrc"]),
            "@acme:registry=https://npm.acme.test/\n",
        );
        assert!(fake.ask(Ecosystem::Npm, "@acme/tool"));
        assert!(!fake.ask(Ecosystem::Npm, "@other/tool"));
        assert!(!fake.ask(Ecosystem::Npm, "left-pad"));
    }

    #[test]
    fn npm_scope_line_naming_the_public_host_is_not_own() {
        let fake = Fake::default().with_file(
            at("ws", &[".npmrc"]),
            "@acme:registry=https://registry.npmjs.org/\n",
        );
        assert!(!fake.ask(Ecosystem::Npm, "@acme/tool"));
    }

    #[test]
    fn npm_default_registry_line_in_project_or_home() {
        let project =
            Fake::default().with_file(at("ws", &[".npmrc"]), "registry=https://npm.acme.test/\n");
        assert!(project.ask(Ecosystem::Npm, "left-pad"));
        assert!(project.ask(Ecosystem::Npm, "@acme/tool"));

        let home =
            Fake::default().with_file(at("home", &[".npmrc"]), "registry=https://npm.acme.test/\n");
        assert!(home.ask(Ecosystem::Npm, "left-pad"));

        let public = Fake::default().with_file(
            at("ws", &[".npmrc"]),
            "registry=https://registry.npmjs.org/\n",
        );
        assert!(!public.ask(Ecosystem::Npm, "left-pad"));
    }

    #[test]
    fn npm_token_lines_are_never_read() {
        let only_token = Fake::default().with_file(
            at("ws", &[".npmrc"]),
            "//npm.acme.test/:_authToken=X\n@acme:_authToken=X\n",
        );
        assert!(!only_token.ask(Ecosystem::Npm, "@acme/tool"));

        let with_public = Fake::default().with_file(
            at("ws", &[".npmrc"]),
            "//npm.acme.test/:_authToken=X\nregistry=https://registry.npmjs.org/\n",
        );
        assert!(!with_public.ask(Ecosystem::Npm, "left-pad"));
        assert_eq!(
            with_public.opened(),
            vec![
                at("ws", &[".npmrc"]),
                at("home", &[".npmrc"]),
                at("ws", &[".yarnrc.yml"]),
            ]
        );
    }

    #[test]
    fn npm_env_decides_before_any_file_is_opened() {
        let own = Fake::default().with_env("NPM_CONFIG_REGISTRY", "https://npm.acme.test/");
        assert!(own.ask(Ecosystem::Npm, "left-pad"));
        assert!(own.opened().is_empty());

        let lower = Fake::default().with_env("npm_config_registry", "https://npm.acme.test/");
        assert!(lower.ask(Ecosystem::Npm, "left-pad"));

        let public = Fake::default().with_env("NPM_CONFIG_REGISTRY", "https://registry.npmjs.org/");
        assert!(!public.ask(Ecosystem::Npm, "left-pad"));
    }

    #[test]
    fn yarnrc_server_and_scopes() {
        let server = Fake::default().with_file(
            at("ws", &[".yarnrc.yml"]),
            "npmRegistryServer: \"https://npm.acme.test\"\n",
        );
        assert!(server.ask(Ecosystem::Npm, "left-pad"));

        let public = Fake::default().with_file(
            at("ws", &[".yarnrc.yml"]),
            "npmRegistryServer: https://registry.npmjs.org\n",
        );
        assert!(!public.ask(Ecosystem::Npm, "left-pad"));

        let scoped = Fake::default().with_file(
            at("ws", &[".yarnrc.yml"]),
            "npmScopes:\n  acme:\n    npmAlwaysAuth: true\n",
        );
        assert!(scoped.ask(Ecosystem::Npm, "@acme/tool"));
        assert!(!scoped.ask(Ecosystem::Npm, "@other/tool"));
        assert!(!scoped.ask(Ecosystem::Npm, "left-pad"));
    }

    #[test]
    fn every_pypi_env_var_counts() {
        for key in PYPI_ENV {
            let own = Fake::default().with_env(key, "https://pypi.acme.test/simple");
            assert!(
                own.ask(Ecosystem::Pypi, "requests"),
                "{key} should mark an own server"
            );
            let public = Fake::default().with_env(key, "https://pypi.org/simple");
            assert!(
                !public.ask(Ecosystem::Pypi, "requests"),
                "{key} with pypi.org is not own"
            );
        }
        assert!(!Fake::default().ask(Ecosystem::Pypi, "requests"));
    }

    #[test]
    fn pypi_name_form_and_extra_urls() {
        let named = Fake::default().with_env("UV_INDEX", "corp=https://pypi.acme.test/simple");
        assert!(named.ask(Ecosystem::Pypi, "requests"));

        let mixed = Fake::default().with_env(
            "PIP_EXTRA_INDEX_URL",
            "https://pypi.org/simple https://pypi.acme.test/simple",
        );
        assert!(mixed.ask(Ecosystem::Pypi, "requests"));

        let default_name = Fake::default().with_env("UV_INDEX", "pypi=https://pypi.org/simple");
        assert!(!default_name.ask(Ecosystem::Pypi, "requests"));
    }

    #[test]
    fn pip_and_uv_files_name_another_index() {
        let cases: [(PathBuf, &str); 6] = [
            (
                at("ws", &["pip.conf"]),
                "[global]\nindex-url = https://pypi.acme.test/simple\n",
            ),
            (
                at("ws", &["uv.toml"]),
                "[[index]]\nname = \"corp\"\nurl = \"https://pypi.acme.test/simple\"\n",
            ),
            (
                at("ws", &["pyproject.toml"]),
                "[[tool.uv.index]]\nname = \"corp\"\n",
            ),
            (
                at("ws", &["pyproject.toml"]),
                "[[tool.poetry.source]]\nname = \"corp\"\n",
            ),
            (
                at("home", &[".config", "pip", "pip.conf"]),
                "[global]\nextra-index-url: https://pip.acme.test/\n",
            ),
            (
                at("appdata", &["pip", "pip.ini"]),
                "[global]\nindex-url=https://pip.acme.test/\n",
            ),
        ];
        for (path, text) in cases {
            let fake = Fake::default()
                .with_file(path.clone(), text)
                .with_env("APPDATA", "appdata");
            assert!(
                fake.ask(Ecosystem::Pypi, "requests"),
                "{} should mark an own server",
                path.display()
            );
        }

        let public = Fake::default().with_file(
            at("ws", &["pip.conf"]),
            "[global]\nindex-url = https://pypi.org/simple\n",
        );
        assert!(!public.ask(Ecosystem::Pypi, "requests"));

        let plain =
            Fake::default().with_file(at("ws", &["pyproject.toml"]), "[project]\nname = \"x\"\n");
        assert!(!plain.ask(Ecosystem::Pypi, "requests"));
    }

    #[test]
    fn cargo_replace_with_under_crates_io_source() {
        let mirror = Fake::default().with_file(
            at("ws", &[".cargo", "config.toml"]),
            "[source.crates-io]\nreplace-with = \"acme-mirror\"\n\n[source.acme-mirror]\nregistry = \"sparse+https://crates.acme.test/\"\n",
        );
        assert!(mirror.ask(Ecosystem::Crates, "serde"));

        let quoted = Fake::default().with_file(
            at("ws", &[".cargo", "config"]),
            "[source.\"crates-io\"]\nreplace-with = \"acme\"\n",
        );
        assert!(quoted.ask(Ecosystem::Crates, "serde"));

        let other_table = Fake::default().with_file(
            at("ws", &[".cargo", "config.toml"]),
            "[source.other]\nreplace-with = \"x\"\n",
        );
        assert!(!other_table.ask(Ecosystem::Crates, "serde"));

        let empty_value = Fake::default().with_file(
            at("ws", &[".cargo", "config.toml"]),
            "[source.crates-io]\nreplace-with = \"\"\n",
        );
        assert!(!empty_value.ask(Ecosystem::Crates, "serde"));

        let no_section = Fake::default().with_file(
            at("ws", &[".cargo", "config.toml"]),
            "[net]\nreplace-with = \"x\"\n",
        );
        assert!(!no_section.ask(Ecosystem::Crates, "serde"));
    }

    #[test]
    fn go_proxy_off_direct_or_company_proxy_is_own() {
        for proxy in ["off", "direct", "https://goproxy.acme.test"] {
            let fake = Fake::default().with_env("GOPROXY", proxy);
            assert!(
                fake.ask(Ecosystem::Go, "golang.org/x/tools"),
                "GOPROXY={proxy}"
            );
        }
        let public = Fake::default().with_env("GOPROXY", "https://proxy.golang.org,direct");
        assert!(!public.ask(Ecosystem::Go, "golang.org/x/tools"));
        assert!(!Fake::default().ask(Ecosystem::Go, "github.com/acme/tool"));
    }

    #[test]
    fn go_private_patterns_match_by_path_element() {
        let cases = [
            ("*.corp.test", "git.corp.test/tool", true),
            ("*.corp.test", "corp.test/tool", false),
            ("*.corp.test", "a/b.corp.test", false),
            ("github.com/acme", "github.com/acme/tool", true),
            ("github.com/acme", "github.com/acme", true),
            ("github.com/acme", "github.com/acmecorp/tool", false),
            ("github.com/acme/*", "github.com/acme/tool/v2", true),
            ("git[0-9].corp.test", "git3.corp.test/tool", true),
            ("git[0-9].corp.test", "gitx.corp.test/tool", false),
        ];
        for (globs, module, expected) in cases {
            let fake = Fake::default().with_env("GOPRIVATE", globs);
            assert_eq!(
                fake.ask(Ecosystem::Go, module),
                expected,
                "GOPRIVATE={globs} module={module}"
            );
        }
    }

    #[test]
    fn go_private_list_and_gonoproxy() {
        let list = Fake::default().with_env("GOPRIVATE", "*.corp.test,github.com/acme");
        assert!(list.ask(Ecosystem::Go, "github.com/acme/x"));
        assert!(list.ask(Ecosystem::Go, "git.corp.test/x"));
        assert!(!list.ask(Ecosystem::Go, "github.com/other/x"));

        let nonproxy = Fake::default().with_env("GONOPROXY", "github.com/acme");
        assert!(nonproxy.ask(Ecosystem::Go, "github.com/acme/x"));
        assert!(!nonproxy.ask(Ecosystem::Go, "golang.org/x/tools"));
    }

    #[test]
    fn gems_gemfile_source_and_gemrc_sources() {
        let gemfile = Fake::default().with_file(
            at("ws", &["Gemfile"]),
            "source \"https://gems.acme.test\"\ngem \"rails\"\n",
        );
        assert!(gemfile.ask(Ecosystem::Gems, "rails"));

        let public = Fake::default().with_file(
            at("ws", &["Gemfile"]),
            "source 'https://rubygems.org'\ngem 'rails'\n",
        );
        assert!(!public.ask(Ecosystem::Gems, "rails"));

        let gemrc = Fake::default().with_file(
            at("home", &[".gemrc"]),
            ":sources:\n- https://gems.acme.test/\n",
        );
        assert!(gemrc.ask(Ecosystem::Gems, "rails"));

        let gemrc_public = Fake::default().with_file(
            at("home", &[".gemrc"]),
            ":sources:\n- https://rubygems.org/\n",
        );
        assert!(!gemrc_public.ask(Ecosystem::Gems, "rails"));
    }

    #[test]
    fn ecosystems_without_lookup_are_never_own() {
        let fake = Fake::default()
            .with_env("GOPROXY", "off")
            .with_env("PIP_INDEX_URL", "https://pypi.acme.test/simple");
        for eco in [Ecosystem::Winget, Ecosystem::Choco, Ecosystem::Scoop] {
            assert!(!fake.ask(eco, "git"));
        }
        assert!(fake.opened().is_empty());
    }

    #[test]
    fn config_reads_stop_at_64_kib() {
        let mut long = "# filler\n".repeat(8_000);
        long.push_str("registry=https://npm.acme.test/\n");
        let cut = Fake::default().with_file(at("ws", &[".npmrc"]), &long);
        assert!(!cut.ask(Ecosystem::Npm, "left-pad"));

        let short = "# filler\n".repeat(100) + "registry=https://npm.acme.test/\n";
        let kept = Fake::default().with_file(at("ws", &[".npmrc"]), &short);
        assert!(kept.ask(Ecosystem::Npm, "left-pad"));
    }

    #[test]
    fn host_of_strips_scheme_userinfo_port_and_path() {
        assert_eq!(
            host_of("https://User@NPM.acme.test:4873/path").as_deref(),
            Some("npm.acme.test")
        );
        assert_eq!(host_of("npm.acme.test").as_deref(), Some("npm.acme.test"));
        assert_eq!(
            host_of("\"https://npm.acme.test/\"").as_deref(),
            Some("npm.acme.test")
        );
        assert_eq!(host_of("https://[::1]:4873/").as_deref(), Some("::1"));
        assert_eq!(host_of("   "), None);
    }
}

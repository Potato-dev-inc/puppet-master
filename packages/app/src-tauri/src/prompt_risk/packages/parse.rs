//! Command text to package references (SPEC-n33 section 3.4). Pure: no I/O, never runs the command.
//!
//! Each segment of the command is split into words the way its shell reads them (`command.rs`).
//! Prefixes (`FOO=1`, `sudo -E`, `env`, `time`, `&`) are dropped, `bash -c`, `cmd /c` and
//! `pwsh -Command` are unwrapped up to two levels, and what is left is read with the grammar of the
//! package tool. A tool is recognised by its program name only, never by who asked for the command.

use once_cell::sync::Lazy;
use regex::Regex;

use super::super::command::{split_segments, take_redirects, windows_style, words};
use super::model::{Ecosystem, RawRef, Source, MAX_NAME_LEN, MAX_SPEC_LEN};

/// `bash -c "x"` is level 1 and a wrapper inside `x` is level 2. Nothing deeper is read.
const MAX_WRAPPER_LEVEL: u8 = 2;

/// Words that run the rest of the line as a program.
const WRAPPERS: &[&str] = &["sudo", "doas", "gsudo", "env", "cross-env", "time", "nohup", "call"];
/// Options of a wrapper that take the next word (`sudo -u root`).
const WRAPPER_VALUES: &[&str] = &["-u", "-g", "-C", "-p", "-r", "-t", "-T", "-U", "-h", "--user", "--group", "--unset"];

const NPM_INSTALL: &[&str] = &[
    "install", "i", "add", "in", "ins", "inst", "insta", "instal", "isnt", "isnta", "isntal", "isntall",
];
const NPM_VALUES: &[&str] = &[
    "--registry", "--userconfig", "--prefix", "-C", "--workspace", "-w", "--tag", "--save-prefix", "--cache", "-p",
    "--package", "-c", "--call", "--location", "--loglevel", "--omit", "--include", "--otp", "--before",
    "--script-shell", "--proxy", "--https-proxy", "--cafile", "--globalconfig", "--fetch-timeout", "--fetch-retries",
];
const PNPM_VALUES: &[&str] = &[
    "--filter", "-F", "-C", "--dir", "--registry", "--config.registry", "-p", "--package", "--reporter", "--loglevel",
    "--store-dir", "--cache-dir",
];
const YARN_VALUES: &[&str] = &["--cwd", "--registry", "--network-timeout", "--cache-folder", "--mutex", "--otp"];
const BUN_VALUES: &[&str] =
    &["--cwd", "--registry", "--cache-dir", "--config", "-c", "--preload", "--env-file", "--tsconfig-override"];
const NPX_VALUES: &[&str] = &["-p", "--package", "-c", "--call", "--registry", "--cache", "--userconfig", "--prefix"];

/// pip and uv. The package-valued ones are listed in `PIP_PACKAGE_FLAGS`.
const PIP_VALUES: &[&str] = &[
    "-r", "--requirement", "-c", "--constraint", "-t", "--target", "--prefix", "--root", "--platform",
    "--python-version", "--implementation", "--only-binary", "--no-binary", "--config-settings", "--src",
    "--upgrade-strategy", "-e", "--editable", "-i", "--index-url", "--extra-index-url", "-f", "--find-links",
    "--progress-bar", "--timeout", "--retries", "--trusted-host", "--cert", "--client-cert", "--proxy",
    "--cache-dir", "--log", "--exists-action", "--report", "--abi", "--python", "-p", "--group", "--optional",
    "--extra", "--with", "--from", "-P", "--upgrade-package", "--index", "--default-index", "--index-strategy",
    "--keyring-provider", "--config-file", "--spec", "--suffix", "--pip-args",
];
/// Flags whose value is a package (`uv tool run --from httpie http`).
const PIP_PACKAGE_FLAGS: &[&str] = &["--with", "--from", "-P", "--upgrade-package", "--spec"];
const PIPX_VALUES: &[&str] = &["--spec", "--python", "--suffix", "--index-url", "-i", "--pip-args"];
const PROJECT_VALUES: &[&str] = &["--group", "-G", "--source", "--extras", "-E", "--python"];
const CARGO_VALUES: &[&str] = &[
    "--features", "-F", "--rename", "--package", "-p", "--manifest-path", "--target", "--branch", "--tag", "--rev",
    "--version", "--vers", "--root", "--bin", "--profile", "-j", "--jobs", "--registry", "--index", "--git", "--path",
];
const GO_VALUES: &[&str] = &["-modfile"];
const GEM_VALUES: &[&str] = &[
    "-v", "--version", "-i", "--install-dir", "-n", "--bindir", "--platform", "-s", "--source", "-g", "--file",
];
const BUNDLE_VALUES: &[&str] = &[
    "-v", "--version", "-g", "--group", "--require", "--branch", "--ref", "-s", "--source", "--git", "--github",
    "--path",
];
const WINGET_VALUES: &[&str] = &[
    "--id", "--name", "-q", "--query", "-s", "--source", "-v", "--version", "--scope", "-l", "--location",
    "--override", "--custom", "-o", "--log", "-a", "--architecture", "--locale", "-m", "--manifest",
];
const CHOCO_VALUES: &[&str] =
    &["--version", "-s", "--source", "--params", "--package-parameters", "--install-arguments"];
const SCOOP_VALUES: &[&str] = &["-a", "--arch"];

static SPEC_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^[A-Za-z0-9.+~^<>=!*_| -]{1,64}$").expect("spec regex"));

pub fn parse(command: &str) -> Vec<RawRef> {
    let mut out = Vec::new();
    parse_text(command, 0, &mut out);
    out
}

fn parse_text(text: &str, level: u8, out: &mut Vec<RawRef>) {
    for segment in split_segments(text) {
        parse_segment(&segment.text, level, out);
    }
}

fn parse_segment(segment: &str, level: u8, out: &mut Vec<RawRef>) {
    let (kept, _targets) = take_redirects(words(segment, windows_style(segment)));
    let args = skip_prefixes(&kept);
    let Some((first, rest)) = args.split_first() else {
        return;
    };
    let tool = program_name(first);
    match tool.as_str() {
        "bash" | "sh" | "zsh" => unwrap_shell(rest, level, out),
        "cmd" => unwrap_cmd(rest, level, out),
        "powershell" | "pwsh" => unwrap_powershell(rest, level, out),
        _ => tool_call(&tool, rest, out),
    }
}

/// Drops the words that come before the program: `FOO=1`, `&`, and wrappers with their options.
fn skip_prefixes(items: &[String]) -> &[String] {
    let mut i = 0;
    while let Some(word) = items.get(i) {
        let lower = word.to_ascii_lowercase();
        if word == "&" || is_assignment(word) {
            i += 1;
        } else if WRAPPERS.contains(&lower.as_str()) {
            i += 1;
            while let Some(option) = items.get(i) {
                if !option.starts_with('-') {
                    break;
                }
                i += 1;
                if WRAPPER_VALUES.contains(&option.as_str()) {
                    i += 1;
                }
            }
        } else {
            break;
        }
    }
    &items[i.min(items.len())..]
}

fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `npm`, `/usr/bin/npm`, `npm.cmd`, `C:\x\npm.exe` -> `npm`.
fn program_name(word: &str) -> String {
    let base = word.rsplit(|c: char| c == '/' || c == '\\').next().unwrap_or(word).to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".ps1"] {
        if let Some(stem) = base.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    base
}

fn unwrap_shell(args: &[String], level: u8, out: &mut Vec<RawRef>) {
    if level >= MAX_WRAPPER_LEVEL {
        return;
    }
    for (i, arg) in args.iter().enumerate() {
        if is_short_c_flag(arg) {
            if let Some(payload) = args.get(i + 1) {
                parse_text(payload, level + 1, out);
            }
            return;
        }
        if !arg.starts_with('-') {
            return;
        }
    }
}

/// `-c`, `-lc`, `-ic`: a short option group that ends in `c`.
fn is_short_c_flag(arg: &str) -> bool {
    arg.len() >= 2 && arg.starts_with('-') && !arg.starts_with("--") && arg.ends_with('c') && arg[1..].chars().all(|c| c.is_ascii_alphabetic())
}

fn unwrap_cmd(args: &[String], level: u8, out: &mut Vec<RawRef>) {
    if level >= MAX_WRAPPER_LEVEL {
        return;
    }
    if let Some(at) = args.iter().position(|arg| matches!(arg.to_ascii_lowercase().as_str(), "/c" | "/k" | "/r")) {
        let payload = args[at + 1..].join(" ");
        parse_text(&payload, level + 1, out);
    }
}

fn unwrap_powershell(args: &[String], level: u8, out: &mut Vec<RawRef>) {
    if level >= MAX_WRAPPER_LEVEL {
        return;
    }
    for (i, arg) in args.iter().enumerate() {
        let lower = arg.to_ascii_lowercase();
        if lower == "-file" || lower == "-f" {
            return;
        }
        // `-c`, `-co`, `-command`, ... are all `-Command`.
        if lower.len() >= 2 && "-command".starts_with(lower.as_str()) {
            let payload = args[i + 1..].join(" ");
            parse_text(&payload, level + 1, out);
            return;
        }
    }
}

fn tool_call(tool: &str, args: &[String], out: &mut Vec<RawRef>) {
    match tool {
        "npm" => node_tool("npm", args, NPM_VALUES, NPM_INSTALL, &["exec", "x"], out),
        "pnpm" => node_tool("pnpm", args, PNPM_VALUES, &["add", "install", "i"], &["dlx"], out),
        "bun" => node_tool("bun", args, BUN_VALUES, &["add", "a", "install", "i"], &["x"], out),
        "yarn" => node_tool("yarn", args, YARN_VALUES, &["add"], &["dlx"], out),
        "npx" | "pnpx" | "bunx" => npx(tool, args, out),
        "uv" => uv(args, out),
        "uvx" => uvx(args, out),
        "pipx" => pipx(args, out),
        "poetry" | "pdm" | "rye" => project_add(tool, args, out),
        "cargo" => cargo(args, out),
        "go" => go(args, out),
        "gem" => gem(args, out),
        "bundle" => bundle(args, out),
        "winget" => winget(args, out),
        "choco" | "cinst" => choco(tool, args, out),
        "scoop" => scoop(args, out),
        _ if is_pip(tool) => pip(tool, args, out),
        _ if is_python(tool) => python(args, out),
        _ => {}
    }
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

fn is_pip(tool: &str) -> bool {
    tool == "pip" || tool == "pip3" || tool.strip_prefix("pip3.").is_some_and(is_digits)
}

fn is_python(tool: &str) -> bool {
    tool == "python" || tool == "python3" || tool == "py" || tool.strip_prefix("python3.").is_some_and(is_digits)
}

// ---- argument reading -------------------------------------------------------------------------

/// The arguments of one tool call, read with its value flags.
#[derive(Default)]
struct Args {
    /// Words that are not flags or flag values, in order. For most tools `ops[0]` is the subcommand.
    ops: Vec<String>,
    /// `(flag, value)` for the flags that take a value, the flag as typed.
    vals: Vec<(String, String)>,
    /// Every other flag, as typed.
    flags: Vec<String>,
}

impl Args {
    fn has(&self, names: &[&str]) -> bool {
        self.flags.iter().any(|flag| names.contains(&flag.as_str()))
    }

    fn vals_of(&self, names: &[&str]) -> Vec<&str> {
        self.vals
            .iter()
            .filter(|(flag, _)| names.contains(&flag.as_str()))
            .map(|(_, value)| value.as_str())
            .collect()
    }
}

/// Reads `args`. A word after `--` is an operand. `--flag=value` is split. A flag in `values`
/// takes the next word, even one that starts with `-`.
fn scan(args: &[String], values: &[&str]) -> Args {
    let mut a = Args::default();
    let mut i = 0;
    let mut ended = false;
    while let Some(word) = args.get(i) {
        i += 1;
        if ended || !word.starts_with('-') || word == "-" {
            a.ops.push(word.clone());
        } else if word == "--" {
            ended = true;
        } else if let Some((flag, value)) = word.split_once('=').filter(|_| word.starts_with("--")) {
            if values.contains(&flag) {
                a.vals.push((flag.to_string(), value.to_string()));
            } else {
                a.flags.push(flag.to_string());
            }
        } else if values.contains(&word.as_str()) {
            let value = args.get(i).cloned().unwrap_or_default();
            i += 1;
            a.vals.push((word.clone(), value));
        } else {
            a.flags.push(word.clone());
        }
    }
    a
}

// ---- what a package reference looks like ------------------------------------------------------

/// One package reference before it is stored.
struct Hit {
    name: String,
    spec: Option<String>,
    source: Source,
}

/// Who installs it: the ecosystem, the tool, and the flags that apply to the whole call.
#[derive(Clone, Copy)]
struct Call<'a> {
    tool: &'a str,
    eco: Ecosystem,
    global: bool,
    runs: bool,
    private: bool,
}

fn emit(out: &mut Vec<RawRef>, call: Call<'_>, hit: Hit) {
    let mut source = match hit.source {
        Source::Registry if call.private => Source::PrivateServer,
        other => other,
    };
    let name = if hit.name.len() > MAX_NAME_LEN {
        // Too long to be a name: kept clipped, and never looked up as one.
        if source == Source::Registry {
            source = Source::NotPlain;
        }
        clip(&hit.name)
    } else {
        hit.name
    };
    if name.is_empty() {
        return;
    }
    let spec = hit.spec.filter(|spec| valid_spec(spec));
    out.push(RawRef {
        ecosystem: call.eco,
        tool: call.tool.to_string(),
        name,
        spec,
        source,
        global: call.global,
        runs_now: call.runs,
    });
}

fn emit_all<'a>(out: &mut Vec<RawRef>, call: Call<'_>, names: impl Iterator<Item = &'a str>, read: fn(&str) -> Hit) {
    for name in names {
        emit(out, call, read(name));
    }
}

fn valid_spec(spec: &str) -> bool {
    spec.len() <= MAX_SPEC_LEN && SPEC_RE.is_match(spec)
}

/// The longest prefix of `text` that fits `MAX_NAME_LEN` bytes, cut on a character boundary.
fn clip(text: &str) -> String {
    let mut end = text.len().min(MAX_NAME_LEN);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].to_string()
}

fn has_shell_chars(text: &str) -> bool {
    text.chars().any(|c| matches!(c, '$' | '%' | '`' | '('))
}

fn not_plain(text: &str) -> Hit {
    Hit { name: clip(text), spec: None, source: Source::NotPlain }
}

fn plain_hit(text: &str) -> Hit {
    if has_shell_chars(text) {
        return not_plain(text);
    }
    Hit { name: text.to_string(), spec: None, source: Source::Registry }
}

fn local_hit(text: &str) -> Hit {
    if has_shell_chars(text) {
        return not_plain(text);
    }
    Hit { name: text.to_string(), spec: None, source: Source::Local }
}

fn with_spec(mut hit: Hit, spec: Option<String>) -> Hit {
    if hit.source == Source::Registry && spec.is_some() {
        hit.spec = spec;
    }
    hit
}

/// `name@spec` -> (`name`, `spec`). A leading `@` is a scope: `@scope/name@spec`.
fn split_name_spec(text: &str) -> (String, Option<String>) {
    let search_from = if text.starts_with('@') { text.find('/').map_or(text.len(), |slash| slash + 1) } else { 0 };
    match text[search_from..].find('@') {
        Some(i) => {
            let at = search_from + i;
            let spec = &text[at + 1..];
            (text[..at].to_string(), (!spec.is_empty()).then(|| spec.to_string()))
        }
        None => (text.to_string(), None),
    }
}

/// Host of a URL, `git+` address, `git@host:path` or `github:` shorthand. Lowercase, no userinfo,
/// no port. Empty when none can be read.
fn host_of(text: &str) -> String {
    let mut lower = text.to_ascii_lowercase();
    for prefix in ["git+", "hg+", "svn+", "bzr+"] {
        if let Some(rest) = lower.strip_prefix(prefix) {
            lower = rest.to_string();
            break;
        }
    }
    for (short, host) in [("github:", "github.com"), ("gitlab:", "gitlab.com"), ("bitbucket:", "bitbucket.org"), ("gist:", "gist.github.com")] {
        if lower.starts_with(short) {
            return host.to_string();
        }
    }
    let rest = match lower.find("://") {
        Some(i) => &lower[i + 3..],
        None => lower.as_str(),
    };
    let authority = rest.split(|c: char| c == '/' || c == '?' || c == '#').next().unwrap_or("");
    let authority = authority.rsplit('@').next().unwrap_or("");
    if let Some(inside) = authority.strip_prefix('[') {
        return inside.split(']').next().unwrap_or("").to_string();
    }
    authority.split(':').next().unwrap_or("").to_string()
}

/// A URL with userinfo, query and fragment removed. The host stays.
fn display_link(text: &str) -> String {
    let head = text.split(|c: char| c == '?' || c == '#').next().unwrap_or("");
    let Some(i) = head.find("://") else {
        return head.to_string();
    };
    let (scheme, rest) = head.split_at(i + 3);
    let authority_end = rest.find('/').unwrap_or(rest.len());
    let (authority, path) = rest.split_at(authority_end);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    clip(&format!("{scheme}{host}{path}"))
}

fn is_url_like(lower: &str) -> bool {
    lower.contains("://") || ["git+", "git@", "hg+", "svn+", "bzr+", "github:", "gitlab:", "bitbucket:", "gist:"].iter().any(|p| lower.starts_with(p))
}

fn is_path_like(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let bytes = text.as_bytes();
    text.starts_with(|c: char| matches!(c, '.' | '/' | '~' | '\\'))
        || (bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':')
        || [".tgz", ".tar.gz", ".tar", ".whl", ".zip", ".gem", ".nupkg"].iter().any(|suffix| lower.ends_with(suffix))
}

/// Where a word points: a link (with its host) or a folder or file on this computer. None for a name.
fn locator(text: &str) -> Option<Source> {
    let lower = text.to_ascii_lowercase();
    if ["file:", "link:", "workspace:"].iter().any(|p| lower.starts_with(p)) {
        return Some(Source::Local);
    }
    if is_url_like(&lower) {
        let host = host_of(text);
        return Some(if host.is_empty() { Source::NotPlain } else { Source::Link { host: clip(&host) } });
    }
    is_path_like(text).then_some(Source::Local)
}

fn locator_hit(text: &str) -> Option<Hit> {
    let source = locator(text)?;
    let name = if matches!(source, Source::Link { .. }) { display_link(text) } else { text.to_string() };
    Some(Hit { name, spec: None, source })
}

fn basename(path: &str) -> &str {
    path.rsplit(|c: char| c == '/' || c == '\\').next().unwrap_or(path)
}

fn is_rubygems(source: &str) -> bool {
    source.trim_end_matches('/').eq_ignore_ascii_case("https://rubygems.org")
}

// ---- npm family -------------------------------------------------------------------------------

/// One npm operand: `name`, `name@spec`, `@scope/name@spec`, `alias@npm:real@spec`, links, paths.
fn npm_operand(op: &str) -> Hit {
    if has_shell_chars(op) {
        return not_plain(op);
    }
    let (name, spec) = split_name_spec(op);
    // `name@url` or `name@file:...`: the address after the `@` is the source. A name with a colon or
    // a leading dot is an address itself (`git+ssh://git@host/...`, `file:../a@b.tgz`), not a name.
    if !name.contains(':') && !name.starts_with('.') {
        if let Some(source) = spec.as_deref().and_then(locator) {
            return Hit { name, spec: None, source };
        }
    }
    if let Some(hit) = locator_hit(op) {
        return hit;
    }
    if let Some(hit) = github_shorthand(op) {
        return hit;
    }
    if let Some(at) = op.find("@npm:") {
        let (name, spec) = split_name_spec(&op[at + 5..]);
        return Hit { name, spec, source: Source::Registry };
    }
    Hit { name, spec, source: Source::Registry }
}

/// `user/repo` (optionally `#ref`) is a GitHub repository, not a package.
fn github_shorthand(op: &str) -> Option<Hit> {
    let base = op.split(|c: char| c == '#' || c == '@').next().unwrap_or("");
    let (owner, repo) = base.split_once('/')?;
    let part_ok = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    (part_ok(owner) && part_ok(repo)).then(|| Hit {
        name: base.to_string(),
        spec: None,
        source: Source::Link { host: "github.com".to_string() },
    })
}

/// npm, pnpm, bun and yarn. `install` lists packages; `exec`, `dlx`, `x` run one.
fn node_tool(tool: &str, args: &[String], values: &[&str], install: &[&str], run: &[&str], out: &mut Vec<RawRef>) {
    let a = scan(args, values);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    let sub = sub.to_ascii_lowercase();
    let call = Call {
        tool,
        eco: Ecosystem::Npm,
        global: a.has(&["-g", "--global"]) || a.vals_of(&["--location"]).contains(&"global"),
        runs: false,
        private: !a.vals_of(&["--registry", "--config.registry", "--userconfig"]).is_empty(),
    };
    if sub == "global" && ops.first().is_some_and(|op| op == "add") {
        // yarn: `yarn global add x`
        emit_all(out, Call { global: true, ..call }, ops[1..].iter().map(String::as_str), npm_operand);
    } else if install.contains(&sub.as_str()) {
        emit_all(out, call, ops.iter().map(String::as_str), npm_operand);
    } else if run.contains(&sub.as_str()) {
        let mut names = a.vals_of(&["-p", "--package"]);
        if names.is_empty() {
            names.extend(ops.first().map(String::as_str));
        }
        emit_all(out, Call { runs: true, global: false, ..call }, names.into_iter(), npm_operand);
    }
}

/// `npx`, `pnpx`, `bunx`: the package is the `-p` value, else the first operand.
fn npx(tool: &str, args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, NPX_VALUES);
    let call = Call {
        tool,
        eco: Ecosystem::Npm,
        global: false,
        runs: true,
        private: !a.vals_of(&["--registry", "--userconfig"]).is_empty(),
    };
    let mut names = a.vals_of(&["-p", "--package"]);
    if names.is_empty() {
        names.extend(a.ops.first().map(String::as_str));
    }
    emit_all(out, call, names.into_iter(), npm_operand);
}

// ---- Python family ----------------------------------------------------------------------------

/// One Python operand: `name[extras]spec`, `name @ url`, links and paths are read by the callers.
fn pep_operand(op: &str) -> Hit {
    if has_shell_chars(op) {
        return not_plain(op);
    }
    if let Some(hit) = locator_hit(op) {
        return hit;
    }
    let head_len = op
        .bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        .count();
    if head_len == 0 {
        return not_plain(op);
    }
    let (name, rest) = op.split_at(head_len);
    let rest = match rest.strip_prefix('[') {
        Some(after) => after.find(']').map_or("", |end| &after[end + 1..]),
        None => rest,
    };
    let spec = rest.trim();
    Hit {
        name: name.to_string(),
        spec: (!spec.is_empty()).then(|| spec.to_string()),
        source: Source::Registry,
    }
}

/// `name @ url`: the name is kept, the address decides the source.
fn direct_hit(name: &str, url: &str) -> Hit {
    let source = if has_shell_chars(url) { Source::NotPlain } else { locator(url).unwrap_or(Source::NotPlain) };
    Hit { name: name.to_string(), spec: None, source }
}

/// Packages of one install, with `name @ url` joined into one reference.
fn pep_list(call: Call<'_>, ops: &[String], out: &mut Vec<RawRef>) {
    let mut i = 0;
    while let Some(op) = ops.get(i) {
        if let Some((name, url)) = op.split_once(" @ ") {
            emit(out, call, direct_hit(name, url));
        } else if ops.get(i + 1).map(String::as_str) == Some("@") && i + 2 < ops.len() {
            emit(out, call, direct_hit(op, &ops[i + 2]));
            i += 2;
        } else {
            emit(out, call, pep_operand(op));
        }
        i += 1;
    }
}

/// `-r file`: the file's basename. Its packages are not read.
fn list_file(value: &str) -> Hit {
    if has_shell_chars(value) {
        return not_plain(value);
    }
    let file = clip(basename(value));
    Hit { name: file.clone(), spec: None, source: Source::ListFile { file } }
}

/// `-e value`: a folder or a link. Anything else is read as a folder.
fn editable(value: &str) -> Hit {
    if has_shell_chars(value) {
        return not_plain(value);
    }
    locator_hit(value).unwrap_or(Hit { name: value.to_string(), spec: None, source: Source::Local })
}

/// True when a package index flag names a server other than pypi.org.
fn index_private(a: &Args) -> bool {
    a.vals.iter().any(|(flag, value)| match flag.as_str() {
        "-i" | "--index-url" | "--index" | "--default-index" => host_of(value) != "pypi.org",
        "--extra-index-url" | "-f" | "--find-links" => true,
        _ => false,
    })
}

/// The flags that name packages, files and folders, and the index server. Returns the call with
/// the index flags applied.
fn pip_flag_refs<'a>(call: Call<'a>, a: &Args, out: &mut Vec<RawRef>) -> Call<'a> {
    let call = Call { private: call.private || index_private(a), ..call };
    for (flag, value) in &a.vals {
        match flag.as_str() {
            "-r" | "--requirement" => emit(out, call, list_file(value)),
            "-e" | "--editable" => emit(out, call, editable(value)),
            flag if PIP_PACKAGE_FLAGS.contains(&flag) => emit(out, call, pep_operand(value)),
            _ => {}
        }
    }
    call
}

/// pip: `pip install ...`. A tool name like `pip3.11` is kept as typed.
fn pip(tool: &str, args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, PIP_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    if !sub.eq_ignore_ascii_case("install") {
        return;
    }
    let call = Call { tool, eco: Ecosystem::Pypi, global: a.has(&["--user"]), runs: false, private: false };
    let call = pip_flag_refs(call, &a, out);
    pep_list(call, ops, out);
}

/// `python -m pip ...` (also `python3`, `python3.N`, `py`).
fn python(args: &[String], out: &mut Vec<RawRef>) {
    let Some(at) = args.iter().position(|arg| arg == "-m") else {
        return;
    };
    if args.get(at + 1).map(String::as_str) != Some("pip") {
        return;
    }
    pip("pip", &args[at + 2..], out);
}

/// A run of a package: `uvx`, `uv tool run`, `pipx run`. The `--from` or `--spec` value is the
/// package; without it the first operand is.
fn run_packages<'a>(call: Call<'a>, a: &Args, ops: &[String], out: &mut Vec<RawRef>) {
    let call = pip_flag_refs(call, a, out);
    if a.vals_of(&["--from", "--spec"]).is_empty() {
        if let Some(first) = ops.first() {
            emit(out, call, pep_operand(first));
        }
    }
}

fn uv(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, PIP_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    let project = Call { tool: "uv", eco: Ecosystem::Pypi, global: false, runs: false, private: false };
    let first = ops.first().map(String::as_str);
    match sub.to_ascii_lowercase().as_str() {
        "add" => {
            let call = pip_flag_refs(project, &a, out);
            pep_list(call, ops, out);
        }
        "pip" if first == Some("install") => {
            let call = pip_flag_refs(Call { global: a.has(&["--system"]), ..project }, &a, out);
            pep_list(call, &ops[1..], out);
        }
        "tool" if first == Some("install") => {
            let call = pip_flag_refs(Call { global: true, ..project }, &a, out);
            pep_list(call, &ops[1..], out);
        }
        "tool" if first == Some("run") => {
            run_packages(Call { runs: true, ..project }, &a, &ops[1..], out);
        }
        _ => {}
    }
}

fn uvx(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, PIP_VALUES);
    let call = Call { tool: "uvx", eco: Ecosystem::Pypi, global: true, runs: true, private: false };
    run_packages(call, &a, &a.ops, out);
}

fn pipx(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, PIPX_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    let base = Call { tool: "pipx", eco: Ecosystem::Pypi, global: true, runs: false, private: false };
    match sub.to_ascii_lowercase().as_str() {
        "install" => {
            let call = pip_flag_refs(base, &a, out);
            pep_list(call, ops, out);
        }
        "run" => run_packages(Call { runs: true, ..base }, &a, ops, out),
        "inject" => {
            // `pipx inject <venv> <package>...`: the venv is the first operand.
            let call = pip_flag_refs(base, &a, out);
            if ops.len() > 1 {
                pep_list(call, &ops[1..], out);
            }
        }
        _ => {}
    }
}

/// poetry, pdm, rye: `add` lists packages. `--source` names another server.
fn project_add(tool: &str, args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, PROJECT_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    if !sub.eq_ignore_ascii_case("add") {
        return;
    }
    let call = Call {
        tool,
        eco: Ecosystem::Pypi,
        global: false,
        runs: false,
        private: !a.vals_of(&["--source"]).is_empty(),
    };
    pep_list(call, ops, out);
}

// ---- cargo, go, gems, Windows and other installers --------------------------------------------

/// A name read with a source flag (`--git`, `--path`, `--github`) takes that source. Without one, a
/// link or a folder is read as such, and anything else as a name.
fn named(op: &str, source: Option<&Source>) -> Hit {
    if has_shell_chars(op) {
        return not_plain(op);
    }
    match source {
        Some(source) => {
            let (name, spec) = split_name_spec(op);
            Hit { name, spec, source: source.clone() }
        }
        None => locator_hit(op).unwrap_or_else(|| {
            let (name, spec) = split_name_spec(op);
            Hit { name, spec, source: Source::Registry }
        }),
    }
}

/// A source flag with no name after it (`cargo install --git <url>`): the source is the package.
fn alone(value: &str) -> Hit {
    locator_hit(value).unwrap_or_else(|| not_plain(value))
}

fn cargo(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, CARGO_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    let sub = sub.to_ascii_lowercase();
    if sub != "add" && sub != "install" {
        return;
    }
    let call = Call {
        tool: "cargo",
        eco: Ecosystem::Crates,
        global: sub == "install",
        runs: false,
        private: !a.vals_of(&["--registry", "--index"]).is_empty(),
    };
    let git = a.vals_of(&["--git"]).first().copied();
    let path = a.vals_of(&["--path"]).first().copied();
    let source = match (git, path) {
        (Some(url), _) => Some(locator(url).unwrap_or(Source::NotPlain)),
        (None, Some(_)) => Some(Source::Local),
        (None, None) => None,
    };
    if ops.is_empty() {
        for value in [git, path].into_iter().flatten() {
            emit(out, call, alone(value));
        }
        return;
    }
    for op in ops {
        emit(out, call, named(op, source.as_ref()));
    }
}

/// `go get` and `go install`. `.`, `./...`, `all`, paths, and names whose first element has no dot
/// (`fmt`) are not module paths and are skipped.
fn go(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, GO_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    let sub = sub.to_ascii_lowercase();
    if sub != "get" && sub != "install" {
        return;
    }
    let call = Call { tool: "go", eco: Ecosystem::Go, global: sub == "install", runs: false, private: false };
    for op in ops {
        if has_shell_chars(op) {
            emit(out, call, not_plain(op));
            continue;
        }
        if op == "all" || op.starts_with('.') || op.starts_with('/') || op.starts_with('\\') {
            continue;
        }
        let (name, spec) = match op.split_once('@') {
            Some((name, spec)) => (name.to_string(), Some(spec.to_string())),
            None => (op.clone(), None),
        };
        if !name.split('/').next().unwrap_or("").contains('.') {
            continue;
        }
        emit(out, call, Hit { name, spec, source: Source::Registry });
    }
}

/// `gem install`. `-v` is the version for every name. `-g` is a Gemfile.
fn gem(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, GEM_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    if !matches!(sub.to_ascii_lowercase().as_str(), "install" | "i") {
        return;
    }
    let call = Call {
        tool: "gem",
        eco: Ecosystem::Gems,
        global: true,
        runs: false,
        private: a.vals_of(&["-s", "--source"]).iter().any(|s| !is_rubygems(s)),
    };
    for file in a.vals_of(&["-g", "--file"]) {
        emit(out, call, list_file(file));
    }
    let spec = a.vals_of(&["-v", "--version"]).first().map(|v| v.to_string());
    for op in ops {
        let hit = locator_hit(op).unwrap_or_else(|| plain_hit(op));
        emit(out, call, with_spec(hit, spec.clone()));
    }
}

/// `bundle add`. `--git`, `--github` and `--path` are the source of the names.
fn bundle(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, BUNDLE_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    if !sub.eq_ignore_ascii_case("add") {
        return;
    }
    let call = Call {
        tool: "bundle",
        eco: Ecosystem::Gems,
        global: false,
        runs: false,
        private: a.vals_of(&["-s", "--source"]).iter().any(|s| !is_rubygems(s)),
    };
    let git = a.vals_of(&["--git"]).first().copied();
    let github = a.vals_of(&["--github"]).first().copied();
    let path = a.vals_of(&["--path"]).first().copied();
    let source = if let Some(url) = git {
        Some(locator(url).unwrap_or(Source::NotPlain))
    } else if github.is_some() {
        Some(Source::Link { host: "github.com".to_string() })
    } else if path.is_some() {
        Some(Source::Local)
    } else {
        None
    };
    if ops.is_empty() {
        if let Some(url) = git {
            emit(out, call, alone(url));
        }
        if let Some(repo) = github {
            emit(out, call, Hit { name: repo.to_string(), spec: None, source: Source::Link { host: "github.com".to_string() } });
        }
        if let Some(dir) = path {
            emit(out, call, alone(dir));
        }
        return;
    }
    let spec = a.vals_of(&["-v", "--version"]).first().map(|v| v.to_string());
    for op in ops {
        emit(out, call, with_spec(named(op, source.as_ref()), spec.clone()));
    }
}

/// winget: `--id`, `--name`, `-q`, `--query` each name a package. Without them the operands are
/// one query.
fn winget(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, WINGET_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    if !matches!(sub.to_ascii_lowercase().as_str(), "install" | "add" | "i") {
        return;
    }
    let call = Call { tool: "winget", eco: Ecosystem::Winget, global: true, runs: false, private: false };
    let ids = a.vals_of(&["--id", "--name", "-q", "--query"]);
    if ids.is_empty() && !ops.is_empty() {
        emit(out, call, plain_hit(&ops.join(" ")));
    }
    for id in ids {
        emit(out, call, plain_hit(id));
    }
    for manifest in a.vals_of(&["-m", "--manifest"]) {
        emit(out, call, local_hit(manifest));
    }
}

/// Chocolatey (`choco install`, `cinst`). A source other than the community feed is a private server.
fn choco(tool: &str, args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, CHOCO_VALUES);
    let names: Vec<&str> = if tool == "cinst" {
        a.ops.iter().map(String::as_str).collect()
    } else {
        match a.ops.split_first() {
            Some((sub, rest)) if matches!(sub.to_ascii_lowercase().as_str(), "install" | "i") => {
                rest.iter().map(String::as_str).collect()
            }
            _ => return,
        }
    };
    let call = Call {
        tool,
        eco: Ecosystem::Choco,
        global: true,
        runs: false,
        private: a.vals_of(&["-s", "--source"]).iter().any(|s| host_of(s) != "community.chocolatey.org"),
    };
    for name in names {
        if name.eq_ignore_ascii_case("all") {
            continue;
        }
        let hit = if name.to_ascii_lowercase().ends_with(".nupkg") { local_hit(name) } else { plain_hit(name) };
        emit(out, call, hit);
    }
}

/// Scoop: a URL is a link, a `*.json` manifest is a file, anything else is `bucket/app` or `app`.
fn scoop(args: &[String], out: &mut Vec<RawRef>) {
    let a = scan(args, SCOOP_VALUES);
    let Some((sub, ops)) = a.ops.split_first() else {
        return;
    };
    if !sub.eq_ignore_ascii_case("install") {
        return;
    }
    let call = Call { tool: "scoop", eco: Ecosystem::Scoop, global: true, runs: false, private: false };
    for op in ops {
        let hit = match locator_hit(op) {
            Some(hit) => hit,
            None if op.to_ascii_lowercase().ends_with(".json") => local_hit(op),
            None => plain_hit(op),
        };
        emit(out, call, hit);
    }
}

#[cfg(test)]
#[path = "parse_tests.rs"]
mod tests;

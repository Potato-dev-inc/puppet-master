//! The generic rule set: what a parsed command does, in plain words. Every rule is about the
//! command, never about which agent asked.

use super::command::{self, Sep};
use super::paths::{self, Where};
use super::{Finding, RiskLevel};
use once_cell::sync::Lazy;
use regex::Regex;

use RiskLevel::{High, Low, Medium};

/// Rank breaks ties between findings of the same level: the highest rank names the headline.
mod rank {
    pub const PIPE_TO_SHELL: u8 = 100;
    pub const HIDDEN_CODE: u8 = 98;
    pub const SYSTEM: u8 = 95;
    pub const ELEVATED: u8 = 90;
    pub const PUBLISH: u8 = 85;
    pub const HISTORY: u8 = 80;
    pub const SEND_SECRET: u8 = 78;
    pub const GLOBAL_INSTALL: u8 = 70;
    pub const DELETE_OUTSIDE: u8 = 66;
    pub const WRITE_SECRET: u8 = 62;
    pub const WRITE_OUTSIDE: u8 = 60;
    pub const DELETE: u8 = 50;
    pub const READ_SECRET: u8 = 45;
    pub const NETWORK: u8 = 40;
    pub const INSTALL: u8 = 35;
    pub const READ_OUTSIDE: u8 = 30;
    pub const WRITE_INSIDE: u8 = 25;
    pub const OTHER: u8 = 20;
    pub const TESTS: u8 = 12;
    pub const CHECK: u8 = 10;
    pub const READ: u8 = 5;
}

pub struct Ctx<'a> {
    pub workspace: Option<&'a str>,
    pub cwd: Option<String>,
    pub findings: Vec<Finding>,
    /// A command nothing recognised: the result is at least medium.
    pub unknown: bool,
    depth: u8,
}

impl<'a> Ctx<'a> {
    pub fn new(workspace: Option<&'a str>, cwd: Option<&str>) -> Self {
        Self { workspace, cwd: cwd.map(str::to_string), findings: Vec::new(), unknown: false, depth: 0 }
    }

    fn add(&mut self, level: RiskLevel, rank: u8, headline: &str, reason: impl Into<String>) {
        self.findings.push(Finding { level, rank, headline: headline.to_string(), reason: reason.into() });
    }

    fn loc(&self, path: &str) -> Where {
        paths::locate(path, self.cwd.as_deref(), self.workspace)
    }
}

// ---- whole-line patterns -------------------------------------------------------------------

static PIPE_TO_SHELL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(curl|wget|iwr|irm|invoke-webrequest|invoke-restmethod|fetch)\b[^|;&\n]*\|\s*(sudo\s+)?(ba|z|da|k)?sh\b|\b(curl|wget|iwr|irm|invoke-webrequest|invoke-restmethod)\b[^|;&\n]*\|\s*(iex|invoke-expression|python3?|node|perl|ruby|pwsh|powershell)\b",
    )
    .unwrap()
});
static IEX_DOWNLOAD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(iex|invoke-expression)\b.{0,200}\b(iwr|irm|invoke-webrequest|invoke-restmethod|curl|wget|downloadstring|downloadfile)\b|\b(downloadstring|iwr|irm|invoke-webrequest|invoke-restmethod)\b.{0,200}\|\s*(iex|invoke-expression)\b|(ba|z)?sh\s+<\(\s*(curl|wget)|(ba|z)?sh\s+-c\s+[\x22']?\$\(\s*(curl|wget)",
    )
    .unwrap()
});
static ENCODED: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)(^|\s)-(e|ec|enc|encodedcommand)\s+[A-Za-z0-9+/=]{12,}").unwrap());
static SECRET_PATH: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?ix)
        (^|[\\/])\.env(\.(local|production|prod|development|dev|staging|test|secret|secrets))?$
        | (^|[\\/])id_(rsa|dsa|ecdsa|ed25519)(\.pub)?$
        | \.(pem|p12|pfx|jks|keystore|ppk)$
        | (^|[\\/])\.aws[\\/]credentials$
        | (^|[\\/])\.ssh([\\/]|$)
        | (^|[\\/])\.(npmrc|netrc|pypirc|git-credentials|pgpass|my\.cnf|vault-token)$
        | (^|[\\/])\.kube[\\/]config$
        | (^|[\\/])\.docker[\\/]config\.json$
        | (^|[\\/])credentials(\.json)?$
        | (^|[\\/])secrets?\.(json|ya?ml|toml)$
        | service[-_]?account.*\.json$
        | \.(key)$
        ",
    )
    .unwrap()
});

pub fn is_secret_path(word: &str) -> bool {
    let w = word.trim().trim_matches(|c| c == '"' || c == '\'').trim_start_matches('@');
    SECRET_PATH.is_match(w)
}

// ---- entry point ---------------------------------------------------------------------------

pub fn analyze(command: &str, ctx: &mut Ctx) {
    if ctx.depth > 3 {
        ctx.unknown = true;
        return;
    }
    if PIPE_TO_SHELL.is_match(command) || IEX_DOWNLOAD.is_match(command) {
        ctx.add(
            High,
            rank::PIPE_TO_SHELL,
            "Download and run code from the internet",
            "It downloads a script and runs it right away, without anyone reading it first.",
        );
    }
    if ENCODED.is_match(command) {
        ctx.add(High, rank::HIDDEN_CODE, "Run hidden code", "The command is encoded, so you cannot read what it will do.");
    }
    let segments = command::split_segments(command);
    for (i, seg) in segments.iter().enumerate() {
        let next_is_shell = seg.sep == Sep::Pipe
            && segments.get(i + 1).is_some_and(|n| {
                let first = n.text.split_whitespace().next().unwrap_or_default().to_ascii_lowercase();
                matches!(first.as_str(), "sh" | "bash" | "zsh" | "iex" | "invoke-expression" | "powershell" | "pwsh")
            });
        segment(&seg.text, next_is_shell, ctx);
    }
}

/// Executable name without directory and extension, lower case.
fn base_name(word: &str) -> String {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word).to_ascii_lowercase();
    for ext in [".exe", ".cmd", ".bat", ".com"] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem.to_string();
        }
    }
    name
}

fn is_flag(word: &str, windows: bool) -> bool {
    (word.starts_with('-') && word.len() > 1)
        || (windows && word.len() >= 2 && word.starts_with('/') && word[1..].chars().next().is_some_and(|c| c.is_ascii_alphabetic()) && !word[1..].contains('/'))
}

fn segment(text: &str, piped_into_shell: bool, ctx: &mut Ctx) {
    let windows = command::windows_style(text);
    let (mut words, redirects) = command::take_redirects(command::words(text, windows));
    // Leading VAR=value assignments and transparent wrappers.
    loop {
        let Some(first) = words.first() else { return };
        let lower = first.to_ascii_lowercase();
        let assignment = first.contains('=') && !first.starts_with('-') && first.split('=').next().is_some_and(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if assignment || matches!(lower.as_str(), "&" | "call" | "time" | "nohup" | "exec" | "command" | "env" | "nice" | "cross-env" | "builtin") {
            words.remove(0);
        } else {
            break;
        }
    }
    let Some(first) = words.first().cloned() else { return };
    let prog = base_name(&first);
    let args: Vec<String> = words[1..].to_vec();

    for target in &redirects {
        write_target(ctx, target, "writes to");
    }
    if piped_into_shell {
        ctx.add(
            High,
            rank::PIPE_TO_SHELL,
            "Download and run code from the internet",
            "Its output is fed straight into a shell and run.",
        );
    }
    // A path that holds secrets, whatever the command does with it.
    let secrets: Vec<&String> = args.iter().filter(|a| is_secret_path(a)).collect();

    classify(&prog, &args, windows, ctx);

    if !secrets.is_empty() {
        let names = secrets.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ");
        let net = is_network(&prog) || args.iter().any(|a| a.starts_with("http"));
        if net {
            ctx.add(High, rank::SEND_SECRET, "Send a file that may hold secrets to the internet", format!("It uses {names}, which can hold passwords or keys, together with a network call."));
        } else if is_writer(&prog, &args) || is_delete(&prog) {
            ctx.add(High, rank::WRITE_SECRET, "Change a file that may hold secrets", format!("It changes {names}, which can hold passwords or keys."));
        } else {
            ctx.add(Medium, rank::READ_SECRET, "Read a file that may hold secrets", format!("It opens {names}, which can hold passwords or keys."));
        }
    }
    for target in &redirects {
        if is_secret_path(target) {
            ctx.add(High, rank::WRITE_SECRET, "Change a file that may hold secrets", format!("It writes to {target}, which can hold passwords or keys."));
        }
    }
}

fn write_target(ctx: &mut Ctx, path: &str, verb: &str) {
    match ctx.loc(path) {
        Where::Outside => ctx.add(High, rank::WRITE_OUTSIDE, "Change files outside this project", format!("It {verb} {path}, which is outside this project.")),
        Where::Unknown => ctx.add(Medium, rank::WRITE_INSIDE, "Change files", format!("It {verb} {path}, and the location could not be checked.")),
        Where::Inside => ctx.add(Medium, rank::WRITE_INSIDE, "Change files in this project", format!("It {verb} {path}.")),
    }
}

fn flag_set(args: &[String], windows: bool) -> Vec<String> {
    args.iter().take_while(|a| a.as_str() != "--").filter(|a| is_flag(a, windows)).map(|a| a.to_ascii_lowercase()).collect()
}

fn has_flag(flags: &[String], names: &[&str]) -> bool {
    flags.iter().any(|f| names.contains(&f.as_str()))
}

/// Short-flag cluster contains a letter: `-rf` has `r`.
fn cluster_has(flags: &[String], letter: char) -> bool {
    flags.iter().any(|f| f.starts_with('-') && !f.starts_with("--") && f[1..].contains(letter))
}

fn operands(args: &[String], windows: bool) -> Vec<&String> {
    let mut out = Vec::new();
    let mut literal = false;
    for a in args {
        if !literal && a == "--" {
            literal = true;
            continue;
        }
        if literal || !is_flag(a, windows) {
            out.push(a);
        }
    }
    out
}

fn is_network(prog: &str) -> bool {
    matches!(prog, "curl" | "wget" | "iwr" | "irm" | "invoke-webrequest" | "invoke-restmethod" | "http" | "https" | "httpie")
}

fn is_delete(prog: &str) -> bool {
    matches!(prog, "rm" | "rmdir" | "rd" | "del" | "erase" | "remove-item" | "ri" | "unlink" | "rimraf" | "trash" | "shred")
}

fn is_writer(prog: &str, args: &[String]) -> bool {
    matches!(
        prog,
        "cp" | "copy" | "xcopy" | "robocopy" | "mv" | "move" | "ren" | "rename" | "copy-item" | "move-item" | "rename-item" | "new-item" | "set-content" | "add-content" | "out-file" | "tee" | "touch" | "mkdir" | "md" | "ni" | "ln" | "install" | "tee-object" | "clear-content"
    ) || (prog == "sed" && args.iter().any(|a| a == "-i" || a.starts_with("-i") && !a.starts_with("--") || a == "--in-place"))
}

const READERS: &[&str] = &[
    "ls", "dir", "cat", "type", "head", "tail", "less", "more", "grep", "egrep", "fgrep", "rg", "ag", "fd", "tree", "wc", "stat", "file", "diff", "cmp", "du", "df",
    "which", "where", "whereis", "whoami", "hostname", "date", "uname", "printenv", "sort", "uniq", "cut", "jq", "column", "basename", "dirname", "realpath", "readlink",
    "get-childitem", "gci", "get-content", "gc", "select-string", "sls", "get-item", "gi", "get-location", "gl", "test-path", "get-command", "gcm", "get-process", "gps",
    "get-date", "measure-object", "format-table", "ft", "get-filehash", "findstr", "awk", "sed", "bat", "nl", "od", "xxd", "md5sum", "sha256sum", "get-service", "ver", "vol", "tasklist", "ps", "top", "free", "id",
    "get-help", "man", "help", "tldr", "history", "lsof", "netstat", "ping", "nslookup", "dig", "ipconfig", "ifconfig", "get-alias", "get-member", "format-list", "select-object", "where-object", "sort-object", "group-object", "out-string", "convertto-json",
];
const NEUTRAL: &[&str] = &[
    "cd", "pushd", "popd", "set-location", "sl", "chdir", "pwd", "echo", "write-host", "write-output", "printf", "true", "false", ":", "exit", "clear", "cls", "export", "set", "sleep", "start-sleep", "timeout", "test", "[", "read", "export", "unset", "alias", "source",
];

fn classify(prog: &str, args: &[String], windows: bool, ctx: &mut Ctx) {
    let flags = flag_set(args, windows);
    match prog {
        // directory changes carry the working directory to later segments
        "cd" | "pushd" | "set-location" | "sl" | "chdir" => {
            if let Some(dir) = operands(args, windows).first() {
                if paths::locate(dir, ctx.cwd.as_deref(), ctx.workspace) == Where::Outside && !dir.starts_with('$') {
                    ctx.add(Medium, rank::READ_OUTSIDE, "Work outside this project", format!("It moves to {dir}, which is outside this project."));
                }
                ctx.cwd = Some(join_cwd(ctx.cwd.as_deref().or(ctx.workspace), dir));
            }
        }
        p if NEUTRAL.contains(&p) => {}
        "sudo" | "doas" | "gsudo" | "runas" | "pkexec" => {
            ctx.add(High, rank::ELEVATED, "Run with administrator rights", "It runs with full control of this computer, so it can change anything.");
            if prog != "runas" {
                let mut rest: Vec<String> = args.to_vec();
                // skip sudo's own options (-u user, -E, -H ...)
                while let Some(first) = rest.first() {
                    if first == "-u" || first == "-g" {
                        rest.drain(..rest.len().min(2));
                    } else if first.starts_with('-') {
                        rest.remove(0);
                    } else {
                        break;
                    }
                }
                if !rest.is_empty() {
                    let joined = rest.join(" ");
                    ctx.depth += 1;
                    analyze(&joined, ctx);
                    ctx.depth -= 1;
                }
            }
        }
        "bash" | "sh" | "zsh" | "dash" | "fish" | "ksh" | "cmd" | "powershell" | "pwsh" | "wsl" => shell_wrapper(prog, args, ctx),
        p if is_delete(p) || p == "remove-item" => delete(p, args, windows, ctx),
        "git" => git(args, ctx),
        "npm" | "pnpm" | "yarn" | "bun" | "npx" | "pnpx" | "bunx" | "corepack" => node_tools(prog, args, ctx),
        "cargo" => cargo(args, ctx),
        "go" => go(args, ctx),
        "python" | "python3" | "py" | "python2" => python(args, ctx),
        "pip" | "pip3" | "uv" | "pipx" | "poetry" | "conda" | "mamba" | "pdm" | "rye" => pip_like(prog, args, ctx),
        "dotnet" | "msbuild" | "nuget" => dotnet(args, ctx),
        "make" | "cmake" | "ninja" | "gradle" | "gradlew" | "mvn" | "mvnw" | "just" | "task" | "bazel" => build_tools(prog, args, ctx),
        "docker" | "podman" | "docker-compose" => docker(args, ctx),
        "gh" | "glab" => gh(args, ctx),
        "pytest" | "py.test" | "tox" | "nox" | "vitest" | "jest" | "mocha" | "phpunit" | "rspec" | "ctest" | "behave" | "playwright" | "cypress" | "ava" | "karma" | "bats" => {
            ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else.")
        }
        "tsc" | "eslint" | "prettier" | "black" | "ruff" | "flake8" | "mypy" | "pyright" | "rubocop" | "biome" | "stylelint" | "clippy" | "gofmt" | "rustfmt" | "isort" | "tslint" | "oxlint" | "shellcheck" => {
            ctx.add(Low, rank::CHECK, "Check or format the code", "It checks or tidies the code in this project.")
        }
        "vite" | "next" | "nuxt" | "astro" | "webpack" | "esbuild" | "rollup" | "parcel" | "turbo" | "nx" | "tsx" | "ts-node" | "nodemon" | "serve" | "live-server" => {
            ctx.add(Low, rank::CHECK, "Build or start the project", "It builds or starts this project.")
        }
        "curl" | "wget" | "iwr" | "irm" | "invoke-webrequest" | "invoke-restmethod" | "http" | "https" | "httpie" => network(prog, args, windows, ctx),
        "ssh" | "scp" | "sftp" | "rsync" | "ftp" | "telnet" | "nc" | "ncat" | "netcat" | "mosh" => {
            ctx.add(Medium, rank::NETWORK, "Connect to another computer", "It talks to another computer over the network.")
        }
        "kill" | "pkill" | "killall" | "taskkill" | "stop-process" | "spps" => {
            ctx.add(Medium, rank::OTHER + 5, "Stop a running program", "It stops a program that is running on this computer.")
        }
        "iex" | "invoke-expression" | "eval" => {
            ctx.add(High, rank::HIDDEN_CODE - 10, "Run text as code", "It runs a piece of text as code, so what it does depends on that text.")
        }
        "start-process" | "start" | "saps" => {
            if args.iter().any(|a| a.eq_ignore_ascii_case("runas")) {
                ctx.add(High, rank::ELEVATED, "Run with administrator rights", "It starts a program with full control of this computer.");
            } else {
                ctx.add(Medium, rank::OTHER + 5, "Start a program", "It starts another program.");
            }
        }
        "find" => find(args, windows, ctx),
        "chmod" | "chown" | "chgrp" | "icacls" | "attrib" | "takeown" => {
            let recursive = has_flag(&flags, &["-r", "--recursive", "/t"]) || cluster_has(&flags, 'R');
            let outside = operands(args, windows).iter().any(|a| paths::looks_like_path(a) && ctx.loc(a) == Where::Outside);
            if outside || (recursive && prog != "attrib" && operands(args, windows).iter().any(|a| paths::is_broad_root(a))) {
                ctx.add(High, rank::SYSTEM - 10, "Change permissions outside this project", "It changes who can read or run files that are outside this project.");
            } else {
                ctx.add(Medium, rank::OTHER + 5, "Change file permissions", "It changes who can read or run some files.");
            }
        }
        "format" | "mkfs" | "dd" | "diskpart" | "fdisk" | "parted" | "shutdown" | "reboot" | "halt" | "poweroff" | "stop-computer" | "restart-computer" | "reg" | "set-executionpolicy"
        | "setx" | "netsh" | "sc" | "schtasks" | "crontab" | "iptables" | "ufw" | "systemctl" | "launchctl" | "bcdedit" | "set-mppreference" | "add-mppreference" | "visudo" | "usermod" | "useradd" | "passwd"
        | "net" | "restart-service" | "stop-service" | "set-service" | "new-service" | "mount" | "umount" | "defaults" | "regedit" | "wmic" | "disable-netadapter" | "set-itemproperty" | "new-itemproperty" => {
            let mild = prog == "reg" && args.first().is_some_and(|a| a.eq_ignore_ascii_case("query"));
            if mild {
                ctx.add(Low, rank::READ, "Look at system settings", "It only reads a system setting.");
            } else if prog == "dd" && !args.iter().any(|a| a.starts_with("of=/dev") || a.starts_with("of=\\\\.")) {
                ctx.unknown = true;
            } else {
                ctx.add(High, rank::SYSTEM, "Change system settings", "It changes settings of this computer, outside any project.");
            }
        }
        "install-module" | "install-package" | "install-script" | "install-module-global" => {
            ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds software that every project on this computer can use.")
        }
        "apt" | "apt-get" | "yum" | "dnf" | "pacman" | "brew" | "choco" | "winget" | "scoop" | "apk" | "zypper" | "snap" | "flatpak" | "port" | "nix-env" | "gem" | "cpan" | "luarocks" | "composer" => {
            system_packages(prog, args, ctx)
        }
        "tar" | "zip" | "unzip" | "7z" | "7za" | "gzip" | "gunzip" | "expand-archive" | "compress-archive" => {
            ctx.add(Medium, rank::OTHER, "Pack or unpack files", "It creates or extracts an archive of files.");
            path_words_outside(args, windows, ctx, true);
        }
        "xargs" => ctx.add(Medium, rank::OTHER, "Run a command on many items", "It runs another command once for each item it is given."),
        "node" | "deno" | "ruby" | "perl" | "php" | "java" | "dotnet-script" | "lua" | "bun-run" => {
            if flags.iter().any(|f| f == "--version" || f == "-v" || f == "-version") && args.len() == 1 {
                ctx.add(Low, rank::READ, "Check a version", "It prints a version number.");
            } else {
                ctx.add(Medium, rank::OTHER, "Run a script", "It runs a program written in code; what it does depends on that code.");
                path_words_outside(args, windows, ctx, false);
            }
        }
        p if is_writer(p, args) => writer(p, args, windows, ctx),
        p if READERS.contains(&p) => reader(p, args, windows, ctx),
        _ => {
            if flags.iter().any(|f| f == "--version" || f == "-version" || f == "-v" || f == "--help" || f == "-h") && args.len() == 1 {
                ctx.add(Low, rank::READ, "Check a version", "It prints a version number or help text.");
            } else {
                ctx.unknown = true;
                path_words_outside(args, windows, ctx, false);
            }
        }
    }
}

fn join_cwd(base: Option<&str>, dir: &str) -> String {
    let d = dir.trim_matches(|c| c == '"' || c == '\'');
    let absolute = d.starts_with('/') || d.starts_with('\\') || d.starts_with('~') || d.as_bytes().get(1) == Some(&b':');
    match base {
        Some(b) if !absolute => {
            let sep = if b.contains('\\') && !b.contains('/') { '\\' } else { '/' };
            format!("{}{}{}", b.trim_end_matches(['/', '\\']), sep, d)
        }
        _ => d.to_string(),
    }
}

/// Medium (with a reason) for path-like words that land outside the project. `writes` raises it to high.
fn path_words_outside(args: &[String], windows: bool, ctx: &mut Ctx, writes: bool) {
    for a in operands(args, windows) {
        if paths::looks_like_path(a) && ctx.loc(a) == Where::Outside {
            if writes {
                ctx.add(High, rank::WRITE_OUTSIDE - 5, "Change files outside this project", format!("It touches {a}, which is outside this project."));
            } else {
                ctx.add(Medium, rank::READ_OUTSIDE, "Use files outside this project", format!("It touches {a}, which is outside this project."));
            }
        }
    }
}

fn reader(prog: &str, args: &[String], windows: bool, ctx: &mut Ctx) {
    ctx.add(Low, rank::READ, "Look at files", "It only reads things and does not change them.");
    for a in operands(args, windows) {
        if paths::looks_like_path(a) && ctx.loc(a) == Where::Outside {
            ctx.add(Medium, rank::READ_OUTSIDE, "Look at files outside this project", format!("It reads {a}, which is outside this project."));
        }
    }
    let _ = prog;
}

fn writer(prog: &str, args: &[String], windows: bool, ctx: &mut Ctx) {
    let mut ops: Vec<&String> = operands(args, windows);
    // Named parameters of PowerShell cmdlets: -Path x -Value y
    let mut named: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let l = args[i].to_ascii_lowercase();
        if matches!(l.as_str(), "-path" | "-literalpath" | "-destination" | "-filepath" | "-lp") {
            if let Some(v) = args.get(i + 1) {
                named.push(v.clone());
            }
        }
        i += 1;
    }
    if !named.is_empty() {
        ops = named.iter().collect();
    }
    let sources_too = matches!(prog, "mv" | "move" | "move-item" | "ren" | "rename" | "rename-item");
    let dest_only = matches!(prog, "cp" | "copy" | "xcopy" | "robocopy" | "copy-item" | "install" | "ln") && named.is_empty();
    let mut wrote_outside = false;
    for (i, p) in ops.iter().enumerate() {
        let last = i + 1 == ops.len();
        let is_dest = !dest_only || last;
        if !(paths::looks_like_path(p) || p.chars().all(|c| c.is_alphanumeric() || "._-".contains(c))) {
            continue;
        }
        match ctx.loc(p) {
            Where::Outside if is_dest || sources_too => {
                wrote_outside = true;
                ctx.add(High, rank::WRITE_OUTSIDE, "Change files outside this project", format!("It writes to or moves {p}, which is outside this project."));
            }
            Where::Outside => ctx.add(Medium, rank::READ_OUTSIDE, "Copy files from outside this project", format!("It copies {p}, which is outside this project.")),
            _ => {}
        }
    }
    if !wrote_outside {
        if matches!(prog, "mkdir" | "md" | "touch") {
            ctx.add(Low, rank::WRITE_INSIDE - 5, "Create files or folders", "It creates an empty file or folder in this project.");
        } else {
            ctx.add(Medium, rank::WRITE_INSIDE, "Change files in this project", "It creates, copies or changes files in this project.");
        }
    }
}

const BUILD_DIRS: &[&str] = &["node_modules", "target", "dist", "build", ".next", ".nuxt", "__pycache__", ".cache", "coverage", "out", ".turbo", ".pytest_cache", ".parcel-cache", ".svelte-kit"];

fn delete(prog: &str, args: &[String], windows: bool, ctx: &mut Ctx) {
    let flags = flag_set(args, windows);
    let powershell = prog == "remove-item" || prog == "ri";
    let recursive = prog == "rimraf"
        || has_flag(&flags, &["--recursive", "-recurse", "-r", "/s"])
        || (!powershell && cluster_has(&flags, 'r'))
        || (!powershell && cluster_has(&flags, 'R'))
        || flags.iter().any(|f| f.starts_with("-recu") || f == "-rec");
    // targets
    let mut targets: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        let l = a.to_ascii_lowercase();
        if matches!(l.as_str(), "-path" | "-literalpath" | "-lp") {
            if let Some(v) = args.get(i + 1) {
                targets.push(v);
            }
            i += 2;
            continue;
        }
        if matches!(l.as_str(), "-filter" | "-include" | "-exclude" | "-erroraction" | "-ea" | "-stream") {
            i += 2;
            continue;
        }
        if !is_flag(a, windows) && a != "--" {
            targets.push(a);
        }
        i += 1;
    }
    if targets.is_empty() {
        ctx.unknown = true;
        return;
    }
    let noun = if recursive { "a folder and everything in it" } else { "files" };
    for t in targets {
        let place = if paths::is_broad_root(t) { Where::Outside } else { ctx.loc(t) };
        let last = t.trim_end_matches(['/', '\\']).rsplit(['/', '\\']).next().unwrap_or(t).to_ascii_lowercase();
        match (place, recursive) {
            (Where::Outside, true) => ctx.add(High, rank::DELETE_OUTSIDE, "Delete a folder outside this project", format!("It deletes {t} and everything in it, and that is outside this project.")),
            (Where::Outside, false) => ctx.add(High, rank::DELETE_OUTSIDE - 1, "Delete files outside this project", format!("It deletes {t}, which is outside this project.")),
            (Where::Unknown, _) => ctx.add(Medium, rank::DELETE, "Delete files", format!("It deletes {t}; where that is could not be checked.")),
            (Where::Inside, true) if BUILD_DIRS.contains(&last.as_str()) => {
                ctx.add(Low, rank::DELETE - 20, "Delete build files", format!("It deletes {t}, which is rebuilt by the project."))
            }
            (Where::Inside, true) => ctx.add(Medium, rank::DELETE, "Delete a folder", format!("It deletes {t} and everything in it, and it cannot be undone.")),
            (Where::Inside, false) => ctx.add(Medium, rank::DELETE - 1, "Delete files", format!("It deletes {t} and it cannot be undone.")),
        }
    }
    let _ = noun;
}

fn find(args: &[String], windows: bool, ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let starts: Vec<&String> = args.iter().take_while(|a| !a.starts_with('-') && !a.starts_with('(') && !a.starts_with('!')).collect();
    if lower.iter().any(|a| a == "-delete") {
        let mut targets: Vec<String> = vec!["-r".into()];
        targets.extend(if starts.is_empty() { vec![".".to_string()] } else { starts.iter().map(|s| s.to_string()).collect() });
        delete("rm", &targets, windows, ctx);
    } else if lower.iter().any(|a| matches!(a.as_str(), "-exec" | "-execdir" | "-ok")) {
        ctx.add(Medium, rank::OTHER, "Run a command on each file found", "It runs another command on every file it finds.");
    } else {
        reader("find", args, windows, ctx);
    }
}

fn shell_wrapper(prog: &str, args: &[String], ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let at = lower.iter().position(|a| matches!(a.as_str(), "-c" | "-lc" | "-ic" | "-command" | "-commandwithargs" | "/c" | "/k" | "-cm"));
    if prog == "wsl" && at.is_none() {
        ctx.add(Medium, rank::OTHER, "Run a command in Linux", "It runs a command in the Linux subsystem.");
        return;
    }
    if lower.iter().any(|a| matches!(a.as_str(), "-encodedcommand" | "-enc" | "-ec" | "-e")) {
        ctx.add(High, rank::HIDDEN_CODE, "Run hidden code", "The command is encoded, so you cannot read what it will do.");
        return;
    }
    match at {
        Some(i) if i + 1 < args.len() => {
            let inner = if prog == "cmd" || args[i].starts_with('/') { args[i + 1..].join(" ") } else { args[i + 1..].join(" ") };
            ctx.depth += 1;
            analyze(&inner, ctx);
            ctx.depth -= 1;
        }
        _ => {
            let script = operands(args, true).first().map(|s| s.to_string());
            match script {
                Some(s) => ctx.add(Medium, rank::OTHER, "Run a script", format!("It runs the script {s}; what it does depends on that script.")),
                None => ctx.add(Low, rank::READ, "Open a shell", "It opens a shell and does nothing else."),
            }
        }
    }
}

fn first_sub(args: &[String]) -> Option<(usize, String)> {
    let mut skip = false;
    for (i, a) in args.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        if matches!(a.as_str(), "-C" | "-c" | "--prefix" | "--cwd" | "-w" | "--workspace" | "--filter" | "-F" | "--dir") {
            skip = true;
            continue;
        }
        if !a.starts_with('-') {
            return Some((i, a.to_ascii_lowercase()));
        }
    }
    None
}

fn git(args: &[String], ctx: &mut Ctx) {
    let Some((at, sub)) = first_sub(args) else {
        ctx.add(Low, rank::READ, "Look at git", "It prints git information.");
        return;
    };
    let rest: Vec<String> = args[at + 1..].to_vec();
    let flags: Vec<String> = rest.iter().filter(|a| a.starts_with('-')).map(|a| a.to_ascii_lowercase()).collect();
    let force_flag = flags.iter().any(|f| f == "--force" || f == "--force-with-lease" || f.starts_with("--force-with-lease=") || f == "-f" || (f.starts_with('-') && !f.starts_with("--") && f[1..].contains('f')));
    match sub.as_str() {
        "push" => {
            let plus = rest.iter().any(|a| a.starts_with('+') && a.len() > 1);
            if force_flag || plus || flags.iter().any(|f| f == "--mirror" || f == "--delete") {
                ctx.add(High, rank::HISTORY, "Overwrite history on the remote", "It force-pushes, which can erase other people's commits on the server.");
            } else {
                ctx.add(Medium, rank::NETWORK, "Upload your commits", "It sends your commits to the remote repository.");
            }
        }
        "reset" if flags.iter().any(|f| f == "--hard" || f == "--merge" || f == "--keep") => {
            ctx.add(High, rank::HISTORY, "Throw away your latest changes", "It resets the project and drops changes that were not saved in a commit.")
        }
        "reset" => ctx.add(Medium, rank::OTHER + 5, "Undo staged changes", "It changes what is staged for the next commit."),
        "clean" => {
            if flags.iter().any(|f| f == "-n" || f == "--dry-run") {
                ctx.add(Low, rank::READ, "Preview files git would delete", "It only lists what it would delete.");
            } else if flags.iter().any(|f| f.contains('x') && !f.starts_with("--")) {
                ctx.add(High, rank::HISTORY, "Delete every untracked and ignored file", "It deletes all files git does not track, including ignored ones such as .env.");
            } else {
                ctx.add(High, rank::HISTORY - 5, "Delete untracked files", "It deletes files that are not saved in git, and they cannot be restored.");
            }
        }
        "filter-branch" | "filter-repo" | "replace" => ctx.add(High, rank::HISTORY, "Rewrite git history", "It rewrites commits across the whole history."),
        "reflog" if rest.iter().any(|a| a == "expire" || a == "delete") => ctx.add(High, rank::HISTORY, "Erase git's safety net", "It deletes the log that lets you recover lost commits."),
        "gc" if flags.iter().any(|f| f.starts_with("--prune")) => ctx.add(High, rank::HISTORY - 5, "Erase git's safety net", "It permanently removes unreferenced commits."),
        "rebase" => ctx.add(Medium, rank::HISTORY - 20, "Rewrite recent commits", "It changes the order or content of your recent commits."),
        "commit" if flags.iter().any(|f| f == "--amend") => ctx.add(Medium, rank::HISTORY - 20, "Rewrite the last commit", "It replaces your last commit."),
        "branch" if flags.iter().any(|f| f == "-d" || f == "--delete" || f == "-D" || f == "-m" || f == "-M") => {
            ctx.add(Medium, rank::DELETE, "Delete or rename a branch", "It removes or renames a git branch.")
        }
        "checkout" | "restore" | "switch" => {
            let discards = rest.iter().any(|a| a == "." || a == "--" || a == ":/") || flags.iter().any(|f| f == "--force" || f == "-f" || f == "--hard");
            if discards && sub != "switch" {
                ctx.add(Medium, rank::DELETE, "Discard your changes", "It throws away edits that were not committed.");
            } else {
                ctx.add(Medium, rank::OTHER, "Switch branch or restore files", "It changes which version of the files is on disk.");
            }
        }
        "stash" if rest.first().is_some_and(|a| a == "drop" || a == "clear") => ctx.add(Medium, rank::DELETE, "Delete saved changes", "It throws away changes you stashed."),
        "merge" | "cherry-pick" | "revert" | "pull" | "am" | "apply" | "bisect" | "submodule" | "worktree" | "tag" if !(sub == "tag" && rest.is_empty()) => {
            ctx.add(Medium, rank::OTHER + 5, "Change the git history", "It brings other commits into this branch or changes the project files.")
        }
        "add" | "commit" | "stash" | "init" | "mv" | "rm" | "notes" | "config" | "remote" | "fetch" | "clone" | "tag" => {
            let (level, headline, reason) = match sub.as_str() {
                "fetch" | "clone" => (Medium, "Download from git", "It downloads data from a remote repository."),
                "rm" | "mv" => (Medium, "Change files tracked by git", "It removes or moves files tracked by git."),
                "config" if rest.iter().any(|a| a == "--global" || a == "--system") => (Medium, "Change git settings for this computer", "It changes git settings for every project."),
                "add" | "commit" | "stash" | "init" => (Low, "Save your changes in git", "It records your work in git on this computer."),
                _ => (Low, "Look at git", "It reads or tidies git information."),
            };
            ctx.add(level, rank::OTHER - 5, headline, reason);
        }
        "status" | "diff" | "log" | "show" | "blame" | "rev-parse" | "ls-files" | "describe" | "shortlog" | "grep" | "branch" | "ls-remote" | "cat-file" | "show-ref" | "rev-list" | "var" | "version" | "help" | "for-each-ref" | "diff-tree" | "name-rev" | "whatchanged" | "reflog" | "fsck" | "count-objects" => {
            ctx.add(Low, rank::READ, "Look at git history", "It only reads git information.")
        }
        _ => ctx.unknown = true,
    }
}

const TEST_SCRIPTS: &[&str] = &["test", "tests", "t", "tst", "e2e", "spec", "vitest", "jest", "test:unit", "test:e2e", "test:ci", "test:watch", "test:run", "ci"];

fn script_kind(name: &str) -> Option<(&'static str, &'static str)> {
    let n = name.to_ascii_lowercase();
    if n.starts_with("test") || TEST_SCRIPTS.contains(&n.as_str()) || n.ends_with(":test") {
        Some(("Run tests", "It runs your project's tests and does not change anything else."))
    } else if n.starts_with("lint") || n.starts_with("check") || n.starts_with("typecheck") || n.starts_with("type-check") || n.starts_with("format") || n == "fmt" || n.starts_with("tsc") || n.starts_with("types") {
        Some(("Check the code", "It checks or tidies the code in this project."))
    } else if n.starts_with("build") || n == "compile" || n == "prepare" || n.starts_with("bundle") {
        Some(("Build the project", "It builds this project and writes the result inside it."))
    } else if matches!(n.as_str(), "dev" | "start" | "serve" | "preview" | "watch" | "storybook" | "develop" | "run" | "web") || n.starts_with("dev:") || n.starts_with("start:") {
        Some(("Start the project", "It starts this project on your computer."))
    } else {
        None
    }
}

fn node_tools(prog: &str, args: &[String], ctx: &mut Ctx) {
    let flags: Vec<String> = args.iter().filter(|a| a.starts_with('-')).map(|a| a.to_ascii_lowercase()).collect();
    let global = flags.iter().any(|f| matches!(f.as_str(), "-g" | "--global" | "--location=global")) || args.first().is_some_and(|a| a == "global");
    let Some((at, sub)) = first_sub(args) else {
        if flags.iter().any(|f| f == "--version" || f == "-v") {
            ctx.add(Low, rank::READ, "Check a version", "It prints a version number.");
        } else {
            ctx.unknown = true;
        }
        return;
    };
    let rest = &args[at + 1..];
    if matches!(prog, "npx" | "pnpx" | "bunx") || sub == "dlx" || (sub == "exec" && prog != "npm") {
        let pkg = if sub == "dlx" || sub == "exec" { rest.iter().find(|a| !a.starts_with('-')) } else { Some(&args[at]) };
        let name = pkg.map(|p| p.to_ascii_lowercase()).unwrap_or_default();
        let base = name.split('@').next().unwrap_or("");
        let base = if name.starts_with('@') { name.as_str() } else { base };
        match base {
            "vitest" | "jest" | "playwright" | "mocha" | "cypress" => ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else."),
            "tsc" | "eslint" | "prettier" | "biome" | "stylelint" | "oxlint" => ctx.add(Low, rank::CHECK, "Check the code", "It checks or tidies the code in this project."),
            "vite" | "next" | "tsx" | "ts-node" | "nodemon" | "turbo" => ctx.add(Low, rank::CHECK, "Build or start the project", "It builds or starts this project."),
            _ => ctx.add(Medium, rank::NETWORK, "Download and run a package", "It fetches a package from the internet and runs it."),
        }
        return;
    }
    match sub.as_str() {
        "publish" | "unpublish" => ctx.add(High, rank::PUBLISH, "Publish to a public registry", "It makes this package public, and published versions are hard to take back."),
        "deprecate" | "login" | "adduser" | "token" | "owner" | "access" | "dist-tag" | "team" | "whoami" if sub != "whoami" => {
            ctx.add(High, rank::PUBLISH - 10, "Change your npm account", "It changes who can publish packages or what is public.")
        }
        "install" | "i" | "add" | "update" | "upgrade" | "up" | "link" | "remove" | "uninstall" | "rm" | "ci" | "dedupe" | "prune" | "rebuild" | "unlink" | "import" | "patch" => {
            if global && !matches!(sub.as_str(), "ci") {
                ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds or changes a tool that every project on this computer can use.");
            } else if matches!(sub.as_str(), "remove" | "uninstall" | "rm" | "prune" | "unlink") {
                ctx.add(Medium, rank::INSTALL, "Remove packages", "It removes packages from this project.");
            } else {
                ctx.add(Medium, rank::INSTALL, "Install packages for this project", "It downloads packages from the internet and may run their setup scripts.");
            }
        }
        "test" | "t" | "tst" => ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else."),
        "start" | "stop" | "restart" => ctx.add(Low, rank::CHECK, "Start the project", "It starts this project on your computer."),
        "view" | "info" | "show" | "ls" | "list" | "outdated" | "why" | "explain" | "search" | "docs" | "help" | "ping" | "doctor" | "pack" | "root" | "bin" | "prefix" | "audit" | "licenses" | "version" | "config" | "get" | "cache" => {
            if sub == "audit" && rest.iter().any(|a| a == "fix") {
                ctx.add(Medium, rank::INSTALL, "Fix package problems", "It changes packages to fix known problems.");
            } else {
                ctx.add(Low, rank::READ, "Look up package info", "It reads package information and changes nothing in your code.");
            }
        }
        "run" | "run-script" | "exec" | "x" | "rum" | "urn" => {
            let script = rest.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_default();
            script_or_other(&script, ctx);
        }
        other => script_or_other(other, ctx),
    }
}

fn script_or_other(script: &str, ctx: &mut Ctx) {
    match script_kind(script) {
        Some((headline, reason)) => ctx.add(Low, rank::CHECK, headline, reason),
        None if script.is_empty() => ctx.unknown = true,
        None => ctx.add(Medium, rank::OTHER, "Run a project script", format!("It runs the project's \"{script}\" script; what it does depends on that script.")),
    }
}

fn cargo(args: &[String], ctx: &mut Ctx) {
    let Some((_, sub)) = first_sub(args) else {
        ctx.unknown = true;
        return;
    };
    match sub.as_str() {
        "test" | "t" | "bench" | "nextest" => ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else."),
        "check" | "c" | "build" | "b" | "clippy" | "fmt" | "doc" | "tree" | "metadata" | "version" | "clean" | "fix" | "audit" | "deny" | "udeps" | "expand" => {
            ctx.add(Low, rank::CHECK, "Build or check the project", "It builds or checks this project and writes the result inside it.")
        }
        "run" | "r" | "watch" => ctx.add(Medium, rank::OTHER, "Run the project", "It builds and runs this project."),
        "publish" | "yank" | "login" | "owner" => ctx.add(High, rank::PUBLISH, "Publish to a public registry", "It makes this crate public, and published versions are hard to take back."),
        "install" | "uninstall" => ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds a tool that every project on this computer can use."),
        "add" | "remove" | "update" | "fetch" | "vendor" => ctx.add(Medium, rank::INSTALL, "Change this project's packages", "It changes which packages this project uses."),
        _ => ctx.unknown = true,
    }
}

fn go(args: &[String], ctx: &mut Ctx) {
    match args.first().map(|s| s.as_str()) {
        Some("test") => ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else."),
        Some("build" | "vet" | "fmt" | "list" | "version" | "env" | "doc" | "mod" | "generate") => {
            ctx.add(Low, rank::CHECK, "Build or check the project", "It builds or checks this project.")
        }
        Some("install") => ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds a tool that every project on this computer can use."),
        Some("get") => ctx.add(Medium, rank::INSTALL, "Change this project's packages", "It downloads packages from the internet."),
        Some("run") => ctx.add(Medium, rank::OTHER, "Run the project", "It builds and runs this project."),
        _ => ctx.unknown = true,
    }
}

fn python(args: &[String], ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    if let Some(at) = lower.iter().position(|a| a == "-m") {
        let module = lower.get(at + 1).cloned().unwrap_or_default();
        let rest: Vec<String> = args.iter().skip(at + 2).cloned().collect();
        match module.as_str() {
            "pytest" | "unittest" | "nose" | "tox" | "doctest" => ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else."),
            "pip" | "pipx" | "uv" => pip_like(&module, &rest, ctx),
            "black" | "ruff" | "flake8" | "mypy" | "isort" | "pyright" | "pylint" | "compileall" => ctx.add(Low, rank::CHECK, "Check or format the code", "It checks or tidies the code in this project."),
            "venv" | "virtualenv" => ctx.add(Medium, rank::OTHER, "Create a Python environment", "It creates a folder with its own copy of Python."),
            "http.server" => ctx.add(Medium, rank::OTHER, "Share a folder on your network", "It serves the files of a folder over the network."),
            _ => ctx.add(Medium, rank::OTHER, "Run a script", "It runs Python code; what it does depends on that code."),
        }
    } else if lower.iter().any(|a| a == "--version" || a == "-v" || a == "-vv") && args.len() == 1 {
        ctx.add(Low, rank::READ, "Check a version", "It prints a version number.");
    } else if lower.iter().any(|a| a == "-c") {
        ctx.add(Medium, rank::OTHER, "Run a small Python snippet", "It runs a line of Python code; what it does depends on that code.");
    } else {
        ctx.add(Medium, rank::OTHER, "Run a script", "It runs a Python script; what it does depends on that script.");
        path_words_outside(args, false, ctx, false);
    }
}

fn pip_like(prog: &str, args: &[String], ctx: &mut Ctx) {
    let Some((at, sub)) = first_sub(args) else {
        ctx.unknown = true;
        return;
    };
    let flags: Vec<String> = args.iter().filter(|a| a.starts_with('-')).map(|a| a.to_ascii_lowercase()).collect();
    let rest = &args[at + 1..];
    let tool = prog == "pipx" || (matches!(prog, "uv") && sub == "tool") || (prog == "poetry" && sub == "self");
    match sub.as_str() {
        "list" | "show" | "freeze" | "check" | "search" | "info" | "--version" | "version" | "config" | "debug" | "inspect" | "tree" | "outdated" => {
            ctx.add(Low, rank::READ, "Look up package info", "It reads package information and changes nothing in your code.")
        }
        "publish" | "upload" => ctx.add(High, rank::PUBLISH, "Publish to a public registry", "It makes this package public, and published versions are hard to take back."),
        "run" | "exec" | "shell" => ctx.add(Medium, rank::OTHER, "Run a command", "It runs a command inside the project's Python setup."),
        "install" | "add" | "remove" | "uninstall" | "sync" | "update" | "upgrade" | "lock" | "tool" | "self" | "pip" | "build" | "init" | "create" => {
            if tool || flags.iter().any(|f| f == "--system" || f == "--global") {
                ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds a tool that every project on this computer can use.");
            } else {
                let _ = rest;
                ctx.add(Medium, rank::INSTALL, "Install Python packages", "It downloads packages from the internet into the current Python setup, which other projects may share.");
            }
        }
        _ => ctx.unknown = true,
    }
}

fn dotnet(args: &[String], ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    match lower.first().map(|s| s.as_str()) {
        Some("test") => ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else."),
        Some("build" | "format" | "--version" | "--info" | "list" | "clean" | "msbuild") => ctx.add(Low, rank::CHECK, "Build or check the project", "It builds or checks this project."),
        Some("tool") if lower.iter().any(|a| a == "-g" || a == "--global") => {
            ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds a tool that every project on this computer can use.")
        }
        Some("nuget") if lower.iter().any(|a| a == "push") => ctx.add(High, rank::PUBLISH, "Publish to a public registry", "It makes this package public, and published versions are hard to take back."),
        Some("restore" | "add" | "remove") => ctx.add(Medium, rank::INSTALL, "Change this project's packages", "It downloads packages from the internet."),
        Some("run" | "publish" | "pack" | "watch") => ctx.add(Medium, rank::OTHER, "Run or package the project", "It builds and runs or packages this project."),
        _ => ctx.unknown = true,
    }
}

fn build_tools(prog: &str, args: &[String], ctx: &mut Ctx) {
    let targets: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).map(|a| a.to_ascii_lowercase()).collect();
    if targets.iter().any(|t| matches!(t.as_str(), "publish" | "deploy" | "release" | "upload" | "publishtomavencentral")) {
        ctx.add(High, rank::PUBLISH, "Publish or deploy", "It sends this project to a registry or a server.");
    } else if targets.iter().any(|t| t.starts_with("test") || t == "check" || t == "verify") {
        ctx.add(Low, rank::TESTS, "Run tests", "It runs your project's tests and does not change anything else.");
    } else if targets.is_empty() || targets.iter().all(|t| matches!(t.as_str(), "build" | "all" | "compile" | "assemble" | "clean" | "package" | "lint" | "fmt" | "format" | "compilejava" | "classes" | "jar")) {
        ctx.add(Low, rank::CHECK, "Build the project", "It builds this project and writes the result inside it.");
    } else if targets.iter().any(|t| matches!(t.as_str(), "install" | "uninstall")) && prog == "make" {
        ctx.add(Medium, rank::INSTALL, "Install what was built", "It copies the built program to places other programs use.");
    } else {
        ctx.add(Medium, rank::OTHER, "Run a build step", format!("It runs the \"{}\" step; what it does depends on the project.", targets.first().cloned().unwrap_or_default()));
    }
}

fn docker(args: &[String], ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let sub = lower.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_default();
    let sub2 = lower.iter().filter(|a| !a.starts_with('-')).nth(1).cloned().unwrap_or_default();
    match sub.as_str() {
        "ps" | "images" | "logs" | "version" | "info" | "inspect" | "stats" | "top" | "diff" | "port" | "history" => ctx.add(Low, rank::READ, "Look at containers", "It only reads information about containers."),
        "push" => ctx.add(High, rank::PUBLISH, "Publish to a public registry", "It uploads an image to a registry."),
        "system" | "volume" | "container" | "image" | "network" if sub2 == "prune" || lower.iter().any(|a| a == "prune") => {
            let all = lower.iter().any(|a| matches!(a.as_str(), "-a" | "--all" | "--volumes"));
            if all || sub == "volume" {
                ctx.add(High, rank::DELETE_OUTSIDE, "Delete container data", "It deletes unused containers, images or volumes, and volumes can hold data you want.");
            } else {
                ctx.add(Medium, rank::DELETE, "Clean up containers", "It deletes unused containers and images.");
            }
        }
        "compose" if matches!(sub2.as_str(), "ps" | "logs" | "config" | "ls" | "version") => ctx.add(Low, rank::READ, "Look at containers", "It only reads information about containers."),
        _ => ctx.add(Medium, rank::OTHER, "Start or change containers", "It builds, starts or changes containers on this computer."),
    }
}

fn gh(args: &[String], ctx: &mut Ctx) {
    let words: Vec<String> = args.iter().filter(|a| !a.starts_with('-')).map(|a| a.to_ascii_lowercase()).collect();
    let (a, b) = (words.first().map(|s| s.as_str()).unwrap_or(""), words.get(1).map(|s| s.as_str()).unwrap_or(""));
    match (a, b) {
        ("release", "create") | ("repo", "delete") | ("repo", "archive") | ("release", "delete") => ctx.add(High, rank::PUBLISH, "Publish or delete on GitHub", "It publishes or removes something on GitHub that other people can see."),
        (_, "view" | "list" | "status" | "checks" | "diff" | "download") | ("auth", "status") | ("status", _) | ("api", _) if a != "api" => {
            ctx.add(Low, rank::READ, "Look at GitHub", "It only reads information from GitHub.")
        }
        _ => ctx.add(Medium, rank::NETWORK, "Post to GitHub", "It creates or changes something on GitHub."),
    }
}

fn system_packages(prog: &str, args: &[String], ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let sub = lower.iter().find(|a| !a.starts_with('-')).cloned().unwrap_or_default();
    match sub.as_str() {
        "list" | "search" | "show" | "info" | "query" | "outdated" | "--version" | "version" | "config" | "env" | "depends" | "why" | "list-installed" | "-q" | "-qa" => {
            ctx.add(Low, rank::READ, "Look up package info", "It reads package information and changes nothing.")
        }
        "install" | "add" | "remove" | "uninstall" | "update" | "upgrade" | "i" | "-s" | "-syu" | "-r" | "-rns" | "dist-upgrade" | "full-upgrade" | "autoremove" | "global" | "require" | "bundle" => {
            if prog == "composer" && !lower.iter().any(|a| a == "global") {
                ctx.add(Medium, rank::INSTALL, "Install packages for this project", "It downloads packages from the internet.");
            } else {
                ctx.add(High, rank::GLOBAL_INSTALL, "Install a package for the whole computer", "It adds, changes or removes software that every project on this computer can use.");
            }
        }
        _ => ctx.add(High, rank::GLOBAL_INSTALL, "Change software on this computer", "It changes software that every project on this computer can use."),
    }
}

fn network(prog: &str, args: &[String], windows: bool, ctx: &mut Ctx) {
    let lower: Vec<String> = args.iter().map(|a| a.to_ascii_lowercase()).collect();
    let url = args.iter().find(|a| a.contains("://") || a.starts_with("localhost") || a.starts_with("127.")).cloned().unwrap_or_default();
    let local = {
        let u = url.to_ascii_lowercase();
        let host = u.split("://").nth(1).unwrap_or(&u);
        host.starts_with("localhost") || host.starts_with("127.0.0.1") || host.starts_with("[::1]") || host.starts_with("0.0.0.0")
    };
    let sends = lower.iter().any(|a| {
        matches!(a.as_str(), "-d" | "--data" | "--data-raw" | "--data-binary" | "--data-urlencode" | "-f" | "--form" | "-t" | "--upload-file" | "--post-data" | "--post-file" | "-body" | "-infile" | "--json")
            || (a.starts_with("-x") && a.len() > 2)
    }) || lower.windows(2).any(|w| (w[0] == "-x" || w[0] == "--request" || w[0] == "-method") && matches!(w[1].as_str(), "post" | "put" | "delete" | "patch"))
        || lower.iter().any(|a| a.starts_with("@") && a.len() > 1);
    let saves = lower.iter().any(|a| matches!(a.as_str(), "-o" | "-O" | "--output" | "-outfile" | "--remote-name" | "-p" | "--directory-prefix") || a.starts_with("-o") && a.len() > 2 && !a.starts_with("--"));
    if local {
        ctx.add(Low, rank::NETWORK - 15, "Check a server on this computer", "It talks to a server running on this computer.");
    } else if sends {
        ctx.add(Medium, rank::NETWORK + 2, "Send data to the internet", "It sends data to another computer over the internet.");
    } else if saves {
        ctx.add(Medium, rank::NETWORK, "Download a file from the internet", "It downloads a file and saves it on this computer.");
    } else {
        ctx.add(Medium, rank::NETWORK - 3, "Fetch something from the internet", "It reads a web address on the internet.");
    }
    let _ = (prog, windows);
    for a in operands(args, windows) {
        if (lower.iter().any(|l| l == "-o" || l == "--output" || l == "-outfile")) && paths::looks_like_path(a) && !a.contains("://") && ctx.loc(a) == Where::Outside {
            ctx.add(High, rank::WRITE_OUTSIDE, "Change files outside this project", format!("It saves a download to {a}, which is outside this project."));
        }
    }
}

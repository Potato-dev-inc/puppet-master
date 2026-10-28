//! Tests for `parse.rs` (SPEC-n33 section 3.4): one row per tool form, prefixes and wrappers, and
//! hostile input (long lines, quotes, unicode, flags that take values).

use super::*;

use crate::prompt_risk::packages::model::{Ecosystem, RawRef, Source, MAX_NAME_LEN};
use Ecosystem::*;
use Src::*;

/// Where the expected package comes from, written short.
#[derive(Clone, Copy)]
enum Src {
    Reg,
    Priv,
    Local,
    Link(&'static str),
    File(&'static str),
    NotPlain,
}

impl Src {
    fn source(self) -> Source {
        match self {
            Src::Reg => Source::Registry,
            Src::Priv => Source::PrivateServer,
            Src::Local => Source::Local,
            Src::Link(host) => Source::Link { host: host.to_string() },
            Src::File(file) => Source::ListFile { file: file.to_string() },
            Src::NotPlain => Source::NotPlain,
        }
    }
}

/// One expected package: ecosystem, tool, name, spec, source, global, runs_now.
#[derive(Clone, Copy)]
struct W(Ecosystem, &'static str, &'static str, Option<&'static str>, Src, bool, bool);

impl W {
    fn raw(&self) -> RawRef {
        let W(ecosystem, tool, name, spec, src, global, runs_now) = *self;
        RawRef {
            ecosystem,
            tool: tool.to_string(),
            name: name.to_string(),
            spec: spec.map(String::from),
            source: src.source(),
            global,
            runs_now,
        }
    }
}

/// (command, the packages it installs or runs, in order).
const ROWS: &[(&str, &[W])] = &[
    // ---- npm ----
    ("npm i lodash", &[W(Npm, "npm", "lodash", None, Reg, false, false)]),
    ("npm install lodash@4.17.21", &[W(Npm, "npm", "lodash", Some("4.17.21"), Reg, false, false)]),
    ("npm i @types/node@^20.1.0", &[W(Npm, "npm", "@types/node", Some("^20.1.0"), Reg, false, false)]),
    ("npm add left-pad", &[W(Npm, "npm", "left-pad", None, Reg, false, false)]),
    ("npm i -g typescript", &[W(Npm, "npm", "typescript", None, Reg, true, false)]),
    ("npm install --global typescript", &[W(Npm, "npm", "typescript", None, Reg, true, false)]),
    ("npm i --location=global tsc", &[W(Npm, "npm", "tsc", None, Reg, true, false)]),
    ("npm i -D typescript", &[W(Npm, "npm", "typescript", None, Reg, false, false)]),
    ("npm i --save-dev react@18 --no-audit", &[W(Npm, "npm", "react", Some("18"), Reg, false, false)]),
    ("npm i --registry https://npm.corp.example lodash", &[W(Npm, "npm", "lodash", None, Priv, false, false)]),
    ("npm i --registry=https://npm.corp.example lodash", &[W(Npm, "npm", "lodash", None, Priv, false, false)]),
    ("npm i --userconfig ./rc lodash", &[W(Npm, "npm", "lodash", None, Priv, false, false)]),
    ("npm i --prefix ./tools lodash", &[W(Npm, "npm", "lodash", None, Reg, false, false)]),
    ("npm i --loglevel error lodash", &[W(Npm, "npm", "lodash", None, Reg, false, false)]),
    ("npm i --omit dev lodash", &[W(Npm, "npm", "lodash", None, Reg, false, false)]),
    ("npm i ./packages/local", &[W(Npm, "npm", "./packages/local", None, Local, false, false)]),
    ("npm i ../sibling", &[W(Npm, "npm", "../sibling", None, Local, false, false)]),
    ("npm i file:../x", &[W(Npm, "npm", "file:../x", None, Local, false, false)]),
    ("npm i foo@file:../foo", &[W(Npm, "npm", "foo", None, Local, false, false)]),
    ("npm i /abs/pkg.tgz", &[W(Npm, "npm", "/abs/pkg.tgz", None, Local, false, false)]),
    (r"npm i C:\tools\pkg.tgz", &[W(Npm, "npm", r"C:\tools\pkg.tgz", None, Local, false, false)]),
    ("npm i git+https://github.com/x/y.git", &[W(Npm, "npm", "git+https://github.com/x/y.git", None, Link("github.com"), false, false)]),
    ("npm i git+ssh://git@github.com/x/y.git", &[W(Npm, "npm", "git+ssh://github.com/x/y.git", None, Link("github.com"), false, false)]),
    ("npm i pkg@git+https://github.com/x/y.git", &[W(Npm, "npm", "pkg", None, Link("github.com"), false, false)]),
    ("npm i github:user/repo#v2", &[W(Npm, "npm", "github:user/repo", None, Link("github.com"), false, false)]),
    ("npm i user/repo", &[W(Npm, "npm", "user/repo", None, Link("github.com"), false, false)]),
    ("npm i gitlab:group/proj", &[W(Npm, "npm", "gitlab:group/proj", None, Link("gitlab.com"), false, false)]),
    ("npm i https://cdn.example.com/pkg-1.0.0.tgz", &[W(Npm, "npm", "https://cdn.example.com/pkg-1.0.0.tgz", None, Link("cdn.example.com"), false, false)]),
    (
        "npm i https://user:secret@registry.example.com:8443/pkg.tgz?token=abc",
        &[W(Npm, "npm", "https://registry.example.com:8443/pkg.tgz", None, Link("registry.example.com"), false, false)],
    ),
    ("npm i react-alias@npm:react@^18.2.0", &[W(Npm, "npm", "react", Some("^18.2.0"), Reg, false, false)]),
    ("npm i alias@npm:@scope/pkg@1.0.0", &[W(Npm, "npm", "@scope/pkg", Some("1.0.0"), Reg, false, false)]),
    ("npm i \"pkg@>=1 <2\"", &[W(Npm, "npm", "pkg", Some(">=1 <2"), Reg, false, false)]),
    ("npm i pkg@1.0,2", &[W(Npm, "npm", "pkg", None, Reg, false, false)]),
    ("npm i $PKG", &[W(Npm, "npm", "$PKG", None, NotPlain, false, false)]),
    (r#"npm i "$(curl x)""#, &[W(Npm, "npm", "$(curl x)", None, NotPlain, false, false)]),
    ("npm i \"lodash", &[W(Npm, "npm", "lodash", None, Reg, false, false)]),
    ("npm i 'a b'", &[W(Npm, "npm", "a b", None, Reg, false, false)]),
    ("npm i lodash --registry", &[W(Npm, "npm", "lodash", None, Priv, false, false)]),
    ("npm i -- lodash", &[W(Npm, "npm", "lodash", None, Reg, false, false)]),
    ("npm i ünïcode 包", &[W(Npm, "npm", "ünïcode", None, Reg, false, false), W(Npm, "npm", "包", None, Reg, false, false)]),
    ("npm ci", &[]),
    ("npm install", &[]),
    ("npm run build", &[]),
    ("npm test", &[]),
    ("npm frobnicate lodash", &[]),
    ("npm exec -- cowsay hi", &[W(Npm, "npm", "cowsay", None, Reg, false, true)]),
    ("npm x --yes create-vite@5 my-app", &[W(Npm, "npm", "create-vite", Some("5"), Reg, false, true)]),
    ("npm exec -p cowsay -- cowsay hi", &[W(Npm, "npm", "cowsay", None, Reg, false, true)]),
    ("npm exec --package=cowsay cowsay", &[W(Npm, "npm", "cowsay", None, Reg, false, true)]),
    ("npm exec -c \"echo hi\"", &[]),
    // ---- pnpm ----
    ("pnpm add zod", &[W(Npm, "pnpm", "zod", None, Reg, false, false)]),
    ("pnpm add -g pnpm-check", &[W(Npm, "pnpm", "pnpm-check", None, Reg, true, false)]),
    ("pnpm install express", &[W(Npm, "pnpm", "express", None, Reg, false, false)]),
    ("pnpm i -D vitest@1", &[W(Npm, "pnpm", "vitest", Some("1"), Reg, false, false)]),
    ("pnpm dlx create-vite@5 demo", &[W(Npm, "pnpm", "create-vite", Some("5"), Reg, false, true)]),
    ("pnpm add --filter web zod", &[W(Npm, "pnpm", "zod", None, Reg, false, false)]),
    ("pnpm add --config.registry https://corp.example zod", &[W(Npm, "pnpm", "zod", None, Priv, false, false)]),
    ("pnpm install", &[]),
    // ---- yarn ----
    ("yarn add react --dev", &[W(Npm, "yarn", "react", None, Reg, false, false)]),
    ("yarn global add tsc", &[W(Npm, "yarn", "tsc", None, Reg, true, false)]),
    ("yarn dlx create-react-app app", &[W(Npm, "yarn", "create-react-app", None, Reg, false, true)]),
    ("yarn --cwd web add zod", &[W(Npm, "yarn", "zod", None, Reg, false, false)]),
    // ---- bun ----
    ("bun add zod", &[W(Npm, "bun", "zod", None, Reg, false, false)]),
    ("bun a -g bun-types", &[W(Npm, "bun", "bun-types", None, Reg, true, false)]),
    ("bun x cowsay hi", &[W(Npm, "bun", "cowsay", None, Reg, false, true)]),
    ("bun install", &[]),
    // ---- npx family ----
    ("npx create-vite@5 my-app", &[W(Npm, "npx", "create-vite", Some("5"), Reg, false, true)]),
    ("npx -y cowsay hi", &[W(Npm, "npx", "cowsay", None, Reg, false, true)]),
    ("npx -p cowsay cowsay hi", &[W(Npm, "npx", "cowsay", None, Reg, false, true)]),
    ("npx --cache /tmp/c cowsay", &[W(Npm, "npx", "cowsay", None, Reg, false, true)]),
    ("npx --registry https://corp.example cowsay", &[W(Npm, "npx", "cowsay", None, Priv, false, true)]),
    ("pnpx tsx file.ts", &[W(Npm, "pnpx", "tsx", None, Reg, false, true)]),
    ("bunx prettier --check .", &[W(Npm, "bunx", "prettier", None, Reg, false, true)]),
    // ---- pip, python -m pip ----
    ("pip install requests", &[W(Pypi, "pip", "requests", None, Reg, false, false)]),
    ("pip3 install requests==2.31.0", &[W(Pypi, "pip3", "requests", Some("==2.31.0"), Reg, false, false)]),
    ("pip3.11 install \"django>=4.2\"", &[W(Pypi, "pip3.11", "django", Some(">=4.2"), Reg, false, false)]),
    ("pip install \"requests>=2.0,<3\"", &[W(Pypi, "pip", "requests", None, Reg, false, false)]),
    ("pip install requests[socks]==2.31.0", &[W(Pypi, "pip", "requests", Some("==2.31.0"), Reg, false, false)]),
    ("pip install --user httpie", &[W(Pypi, "pip", "httpie", None, Reg, true, false)]),
    ("pip install -r requirements.txt", &[W(Pypi, "pip", "requirements.txt", None, File("requirements.txt"), false, false)]),
    ("pip install -r ../team/reqs/dev.txt", &[W(Pypi, "pip", "dev.txt", None, File("dev.txt"), false, false)]),
    ("pip install -e .", &[W(Pypi, "pip", ".", None, Local, false, false)]),
    ("pip install -e git+https://github.com/x/y.git#egg=y", &[W(Pypi, "pip", "git+https://github.com/x/y.git", None, Link("github.com"), false, false)]),
    ("pip install -t ./lib requests", &[W(Pypi, "pip", "requests", None, Reg, false, false)]),
    ("pip install --progress-bar off requests", &[W(Pypi, "pip", "requests", None, Reg, false, false)]),
    ("pip install --extra-index-url https://packages.corp.example/simple requests", &[W(Pypi, "pip", "requests", None, Priv, false, false)]),
    ("pip install -i https://pypi.org/simple requests", &[W(Pypi, "pip", "requests", None, Reg, false, false)]),
    ("pip install -i https://packages.corp.example/simple requests", &[W(Pypi, "pip", "requests", None, Priv, false, false)]),
    ("pip install \"name @ https://example.com/pkg.whl\"", &[W(Pypi, "pip", "name", None, Link("example.com"), false, false)]),
    ("pip install name @ https://example.com/pkg.whl", &[W(Pypi, "pip", "name", None, Link("example.com"), false, false)]),
    ("python -m pip install requests", &[W(Pypi, "pip", "requests", None, Reg, false, false)]),
    ("python3 -m pip install --upgrade pip", &[W(Pypi, "pip", "pip", None, Reg, false, false)]),
    ("py -m pip install flask", &[W(Pypi, "pip", "flask", None, Reg, false, false)]),
    ("pip install", &[]),
    ("pip show requests", &[]),
    // ---- uv, uvx ----
    ("uv add requests", &[W(Pypi, "uv", "requests", None, Reg, false, false)]),
    ("uv pip install --system requests", &[W(Pypi, "uv", "requests", None, Reg, true, false)]),
    ("uv pip install --python 3.12 requests", &[W(Pypi, "uv", "requests", None, Reg, false, false)]),
    ("uv tool install ruff", &[W(Pypi, "uv", "ruff", None, Reg, true, false)]),
    ("uv tool run ruff check", &[W(Pypi, "uv", "ruff", None, Reg, false, true)]),
    ("uv tool run --from httpie http", &[W(Pypi, "uv", "httpie", None, Reg, false, true)]),
    ("uv add -r requirements.txt", &[W(Pypi, "uv", "requirements.txt", None, File("requirements.txt"), false, false)]),
    ("uvx ruff check .", &[W(Pypi, "uvx", "ruff", None, Reg, true, true)]),
    ("uvx --from git+https://github.com/x/tool tool-cli", &[W(Pypi, "uvx", "git+https://github.com/x/tool", None, Link("github.com"), true, true)]),
    ("uvx --with rich ruff check", &[W(Pypi, "uvx", "rich", None, Reg, true, true), W(Pypi, "uvx", "ruff", None, Reg, true, true)]),
    ("uvx --python 3.12 black --check", &[W(Pypi, "uvx", "black", None, Reg, true, true)]),
    // ---- pipx ----
    ("pipx install black", &[W(Pypi, "pipx", "black", None, Reg, true, false)]),
    ("pipx run black --check .", &[W(Pypi, "pipx", "black", None, Reg, true, true)]),
    ("pipx run --spec httpie http", &[W(Pypi, "pipx", "httpie", None, Reg, true, true)]),
    ("pipx inject myvenv flake8 pytest", &[W(Pypi, "pipx", "flake8", None, Reg, true, false), W(Pypi, "pipx", "pytest", None, Reg, true, false)]),
    ("pipx install --index-url https://packages.corp.example/simple black", &[W(Pypi, "pipx", "black", None, Priv, true, false)]),
    // ---- poetry, pdm, rye ----
    ("poetry add requests", &[W(Pypi, "poetry", "requests", None, Reg, false, false)]),
    ("poetry add --group dev pytest", &[W(Pypi, "poetry", "pytest", None, Reg, false, false)]),
    ("poetry add --optional httpx", &[W(Pypi, "poetry", "httpx", None, Reg, false, false)]),
    ("poetry add --source corp requests", &[W(Pypi, "poetry", "requests", None, Priv, false, false)]),
    ("pdm add flask", &[W(Pypi, "pdm", "flask", None, Reg, false, false)]),
    ("rye add click", &[W(Pypi, "rye", "click", None, Reg, false, false)]),
    // ---- cargo ----
    ("cargo add serde@1 --features derive", &[W(Crates, "cargo", "serde", Some("1"), Reg, false, false)]),
    ("cargo add tokio --features full,rt", &[W(Crates, "cargo", "tokio", None, Reg, false, false)]),
    ("cargo add serde@=1.0.200", &[W(Crates, "cargo", "serde", Some("=1.0.200"), Reg, false, false)]),
    ("cargo add foo --registry corp", &[W(Crates, "cargo", "foo", None, Priv, false, false)]),
    ("cargo add --path ../shared shared-utils", &[W(Crates, "cargo", "shared-utils", None, Local, false, false)]),
    ("cargo install ripgrep", &[W(Crates, "cargo", "ripgrep", None, Reg, true, false)]),
    ("cargo install --git https://github.com/x/tool.git", &[W(Crates, "cargo", "https://github.com/x/tool.git", None, Link("github.com"), true, false)]),
    ("cargo install --path ./cli", &[W(Crates, "cargo", "./cli", None, Local, true, false)]),
    ("cargo build", &[]),
    // ---- go ----
    ("go get github.com/BurntSushi/toml@v1.3.2", &[W(Go, "go", "github.com/BurntSushi/toml", Some("v1.3.2"), Reg, false, false)]),
    ("go install golang.org/x/tools/gopls@latest", &[W(Go, "go", "golang.org/x/tools/gopls", Some("latest"), Reg, true, false)]),
    ("go install example.com/tool/cmd@v0.1.0", &[W(Go, "go", "example.com/tool/cmd", Some("v0.1.0"), Reg, true, false)]),
    ("go get ./...", &[]),
    ("go get -u ./...", &[]),
    ("go get .", &[]),
    ("go get fmt", &[]),
    ("go test ./...", &[]),
    // ---- gem, bundle ----
    ("gem install rails -v 7.1", &[W(Gems, "gem", "rails", Some("7.1"), Reg, true, false)]),
    ("gem install --version 6.0 nokogiri", &[W(Gems, "gem", "nokogiri", Some("6.0"), Reg, true, false)]),
    ("gem i rake", &[W(Gems, "gem", "rake", None, Reg, true, false)]),
    ("gem install -g Gemfile", &[W(Gems, "gem", "Gemfile", None, File("Gemfile"), true, false)]),
    ("gem install foo -s https://gems.corp.example", &[W(Gems, "gem", "foo", None, Priv, true, false)]),
    ("gem install foo -s https://rubygems.org", &[W(Gems, "gem", "foo", None, Reg, true, false)]),
    ("gem install ./pkg/x.gem", &[W(Gems, "gem", "./pkg/x.gem", None, Local, true, false)]),
    ("bundle add rails --version 7.1", &[W(Gems, "bundle", "rails", Some("7.1"), Reg, false, false)]),
    ("bundle add nokogiri --git https://github.com/sparklemotion/nokogiri.git", &[W(Gems, "bundle", "nokogiri", None, Link("github.com"), false, false)]),
    ("bundle add mygem --github user/mygem", &[W(Gems, "bundle", "mygem", None, Link("github.com"), false, false)]),
    ("bundle add --github user/gem", &[W(Gems, "bundle", "user/gem", None, Link("github.com"), false, false)]),
    ("bundle add foo --path ./vendor/foo", &[W(Gems, "bundle", "foo", None, Local, false, false)]),
    ("bundle install", &[]),
    // ---- winget, choco, scoop ----
    ("winget install --id Git.Git -e", &[W(Winget, "winget", "Git.Git", None, Reg, true, false)]),
    ("winget install Microsoft.PowerToys", &[W(Winget, "winget", "Microsoft.PowerToys", None, Reg, true, false)]),
    ("winget install -q Mozilla.Firefox --scope user", &[W(Winget, "winget", "Mozilla.Firefox", None, Reg, true, false)]),
    ("winget install --manifest ./app.yaml", &[W(Winget, "winget", "./app.yaml", None, Local, true, false)]),
    ("winget install Microsoft Teams", &[W(Winget, "winget", "Microsoft Teams", None, Reg, true, false)]),
    ("winget search git", &[]),
    ("choco install nodejs -y", &[W(Choco, "choco", "nodejs", None, Reg, true, false)]),
    ("choco install nodejs --version 18.0.0", &[W(Choco, "choco", "nodejs", None, Reg, true, false)]),
    ("choco install git -s https://packages.corp.example/choco", &[W(Choco, "choco", "git", None, Priv, true, false)]),
    ("choco install git -s https://community.chocolatey.org/api/v2", &[W(Choco, "choco", "git", None, Reg, true, false)]),
    ("cinst git -y", &[W(Choco, "cinst", "git", None, Reg, true, false)]),
    ("choco install ./pkg.1.0.nupkg", &[W(Choco, "choco", "./pkg.1.0.nupkg", None, Local, true, false)]),
    ("choco install all", &[]),
    ("choco upgrade git", &[]),
    ("scoop install main/git", &[W(Scoop, "scoop", "main/git", None, Reg, true, false)]),
    ("scoop install https://example.com/app.json", &[W(Scoop, "scoop", "https://example.com/app.json", None, Link("example.com"), true, false)]),
    ("scoop install ./app.json", &[W(Scoop, "scoop", "./app.json", None, Local, true, false)]),
    ("scoop install -a 64bit git", &[W(Scoop, "scoop", "git", None, Reg, true, false)]),
    ("scoop search git", &[]),
    // ---- prefixes and wrappers ----
    ("sudo -E npm i -g x", &[W(Npm, "npm", "x", None, Reg, true, false)]),
    ("sudo -u root npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("cd web && pnpm add zod", &[W(Npm, "pnpm", "zod", None, Reg, false, false)]),
    ("bash -c \"npm i x\"", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("sh -lc 'npm i x'", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("pwsh -Command \"npm i x\"", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("powershell -NoProfile -c \"npm i x\"", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("cmd /c \"npm i x\"", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("cmd /c npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("FOO=1 npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("env CI=1 npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("cross-env NODE_ENV=production npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("time npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("npm.cmd i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("NPM.EXE i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    (r#""C:\Program Files\nodejs\npm.cmd" install x"#, &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("& npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("npm i x > log.txt 2>&1", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("echo hi | npm i x", &[W(Npm, "npm", "x", None, Reg, false, false)]),
    ("npm i a; npm i b && npm i c || npm i d", &[
        W(Npm, "npm", "a", None, Reg, false, false),
        W(Npm, "npm", "b", None, Reg, false, false),
        W(Npm, "npm", "c", None, Reg, false, false),
        W(Npm, "npm", "d", None, Reg, false, false),
    ]),
    ("npm i a & npm i b", &[W(Npm, "npm", "a", None, Reg, false, false), W(Npm, "npm", "b", None, Reg, false, false)]),
    ("npm i x\nnpm i y", &[W(Npm, "npm", "x", None, Reg, false, false), W(Npm, "npm", "y", None, Reg, false, false)]),
    (r#"echo "npm i x""#, &[]),
    ("cat <<EOF\nnpm i evil\nEOF", &[]),
    ("# npm i x", &[]),
    ("", &[]),
    // ---- wrapper depth: two levels are read, a third is not ----
    (r#"bash -c 'bash -c "npm i deep2"'"#, &[W(Npm, "npm", "deep2", None, Reg, false, false)]),
    (r#"bash -c 'bash -c "bash -c \"npm i deep3\""'"#, &[]),
];

#[test]
fn table_rows_match() {
    for (command, want) in ROWS {
        let expect: Vec<RawRef> = want.iter().map(W::raw).collect();
        assert_eq!(parse(command), expect, "command: {command}");
    }
}

#[test]
fn table_has_at_least_sixty_rows() {
    assert!(ROWS.len() >= 60, "only {} rows", ROWS.len());
}

#[test]
fn segment_cap_holds_at_forty() {
    let command = (0..41).map(|i| format!("npm i p{i}")).collect::<Vec<_>>().join("; ");
    let names: Vec<String> = parse(&command).into_iter().map(|r| r.name).collect();
    assert_eq!(names.len(), 40);
    assert_eq!(names.first().map(String::as_str), Some("p0"));
    assert_eq!(names.last().map(String::as_str), Some("p39"));
}

#[test]
fn very_long_line_is_bounded_and_does_not_panic() {
    let command = format!("npm i {}", "a ".repeat(5000));
    let refs = parse(&command);
    assert!(!refs.is_empty());
    assert!(refs.len() <= 2000, "{} refs", refs.len());
    assert!(refs.iter().all(|r| r.name == "a"));
}

#[test]
fn very_long_name_is_not_plain() {
    let refs = parse(&format!("npm i {}", "a".repeat(300)));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].source, Source::NotPlain);
    assert_eq!(refs[0].name.len(), MAX_NAME_LEN);
}

#[test]
fn very_long_spec_is_dropped() {
    let refs = parse(&format!("npm i x@{}", "1".repeat(100)));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, "x");
    assert_eq!(refs[0].spec, None);
}

#[test]
fn very_long_local_path_is_clipped() {
    let refs = parse(&format!("npm i ./{}", "x/".repeat(200)));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].source, Source::Local);
    assert!(refs[0].name.len() <= MAX_NAME_LEN);
}

#[test]
fn unicode_long_name_is_cut_on_a_character_boundary() {
    let name = "é".repeat(150);
    let refs = parse(&format!("npm i {name}"));
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].source, Source::NotPlain);
    assert!(refs[0].name.len() <= MAX_NAME_LEN);
    assert!(refs[0].name.chars().all(|c| c == 'é'));
}

#[test]
fn unicode_link_host_and_name_do_not_panic() {
    let refs = parse("npm i https://例え.テスト/包.tgz");
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, "https://例え.テスト/包.tgz");
}

#[test]
fn values_do_not_become_packages() {
    // A value flag takes its value, so a path or a level name is not read as a package.
    let refs = parse("npm i --prefix ./tools --loglevel verbose lodash");
    assert_eq!(refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["lodash"]);
    let refs = parse("pip install -t ./lib --progress-bar off requests");
    assert_eq!(refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["requests"]);
}

#[test]
fn private_flag_only_changes_its_own_command() {
    let refs = parse("npm i --registry https://corp.example internal; npm i public");
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].source, Source::PrivateServer);
    assert_eq!(refs[1].source, Source::Registry);
}

#[test]
fn quotes_inside_names_are_removed_by_the_shell_rules() {
    let refs = parse(r#"npm i "lodash" 'react'"#);
    assert_eq!(refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["lodash", "react"]);
}

#[test]
fn secret_shaped_text_is_read_as_typed() {
    // Parsing does not redact. The extract step drops secret-shaped names before any card sees them.
    let refs = parse("npm i sk-ant-api03-abcdef");
    assert_eq!(refs[0].name, "sk-ant-api03-abcdef");
}

#[test]
fn nothing_runs_for_non_package_tools() {
    assert!(parse("rm -rf node_modules && ls -la && git status").is_empty());
}

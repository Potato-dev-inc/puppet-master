# Contributing

How the repo is laid out, how to build/test, and where to change things: **[docs/wiki/](docs/wiki/README.md)**. Start with [Developing](docs/wiki/developing.md) and [Extending](docs/wiki/extending.md).

## Local Tool Config

Local agent and editor configuration is intentionally not tracked:

- `.claude/`
- `.codex/`
- `.cursor/`
- `.mcp.json`
- `opencode.json`

Use `.mcp.json.example` or the host-specific examples in `MCP_HOSTS.md` as a starting point, then keep your actual local paths and credentials outside git.

## Faster Rust test runs

CI runs the Rust tests with [cargo-nextest](https://nexte.st), which runs every test in its own
process (config: `packages/app/src-tauri/.config/nextest.toml`, no retries). Locally, from
`packages/app/src-tauri` (`packages/app/dist` must exist: `npm run build --workspace=@puppet-master/app`
once, or `npx vite build` in `packages/app`):

```powershell
cargo nextest run --lib --bins                                   # unit tests
cargo nextest run --features dev-fake-tui --test fake_tui_e2e     # fake-agent end-to-end tests, in parallel
cargo nextest run --features dev-fake-tui --test fake_tui_e2e -E 'test(simple)'   # one of them
```

Plain `cargo test` keeps working for all of them. Each fake-agent test process gets its own app-data
and temp folders (under `%TEMP%\pm-ftui-e2e-<pid>`), so the tests run side by side.

Two opt-in speed-ups for your own shell (nothing in the repo turns them on for you):

- **rust-lld** (the LLVM linker that ships with rustc) links the big test executables; CI uses it:
  `$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER = "rust-lld"`.
- **sccache** shares compiled dependencies between worktrees that each have their own
  `CARGO_TARGET_DIR`, so a new worktree does not rebuild the ~670 dependency crates from scratch.
  Install it yourself (`cargo install sccache --locked`, or a release binary from
  [mozilla/sccache](https://github.com/mozilla/sccache/releases)), then per shell:

  ```powershell
  $env:RUSTC_WRAPPER = "sccache"
  $env:SCCACHE_DIR = "D:\sccache"          # a disk with room; the default is under %LOCALAPPDATA%
  $env:SCCACHE_CACHE_SIZE = "30G"
  $env:CARGO_INCREMENTAL = "0"             # sccache cannot cache incremental builds
  sccache --show-stats                     # hits and misses
  ```

  What is shared is the crates.io dependencies (their sources sit at the same path for every
  worktree); the app crate itself is keyed by its own absolute path and still compiles once per
  worktree. Check `sccache --show-stats` after building a second worktree to see the hits. Leave CI as
  it is: it caches `target/` with `Swatinem/rust-cache` instead.

## Commits

Use scoped conventional commits for new work, for example:

```text
feat(mobile): add pairing retry state
fix(mcp): handle missing bridge port file
docs(readme): update smoke test commands
```

Do not rewrite published history just to clean up older commit messages.

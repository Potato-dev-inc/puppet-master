# Ported code

Code copied from other projects. Each row names the source, the commit, and what changed.

| File (in this repo) | Source path | Commit | Licence | Changes |
|---|---|---|---|---|
| `crates/desk-core/` (all `src/*.rs`, `Cargo.toml`) | `desktoplab/crates/hop-core/` (github.com/v7ren/desktoplab) | 6e5360e | MIT, see `LICENSES/MIT-desktoplab.txt` | Crate renamed `hop-core` to `desk-core`. Workspace deps replaced with concrete versions. Header comment added to each file. No logic change. |
| `crates/desk-proto/` (all `src/*.rs`, `Cargo.toml`) | `desktoplab/crates/hop-proto/` (github.com/v7ren/desktoplab) | 6e5360e | MIT, see `LICENSES/MIT-desktoplab.txt` | Crate renamed `hop-proto` to `desk-proto`. Workspace deps replaced with concrete versions (`postcard` 1 added). Header comment added to each file. No logic change. |
| `src/desk/` (`host.rs`, `engine/{hotkeys,hops,config}.rs`, `os/windows/{cursor,display,dpi,hook}.rs`) | `desktoplab/crates/hop-platform/src/{lib.rs,windows/{capture,cursor,display}.rs}`, `hop-engine/src/{local,hotkeys,config}.rs` (github.com/v7ren/desktoplab) | 6e5360e | MIT, see `LICENSES/MIT-desktoplab.txt` | Behaviour ported, code rewritten (phase DL-3, local hops only). `Host` trait became `DeskHost` (no input injection, clipboard, drag or remote control yet). Keyboard hook only, own thread, per-install receiver, watchdog. Process-wide DPI call replaced by a per-thread scope. The mock fallback and `hop-net` are not ported. DevHop bugs fixed on the way are listed in the commit message and have tests in `src/desk/tests.rs`. |

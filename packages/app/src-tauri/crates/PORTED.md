# Ported code

Code copied from other projects. Each row names the source, the commit, and what changed.

| File (in this repo) | Source path | Commit | Licence | Changes |
|---|---|---|---|---|
| `crates/desk-core/` (all `src/*.rs`, `Cargo.toml`) | `desktoplab/crates/hop-core/` (github.com/v7ren/desktoplab) | 6e5360e | MIT, see `LICENSES/MIT-desktoplab.txt` | Crate renamed `hop-core` to `desk-core`. Workspace deps replaced with concrete versions. Header comment added to each file. No logic change. |
| `crates/desk-proto/` (all `src/*.rs`, `Cargo.toml`) | `desktoplab/crates/hop-proto/` (github.com/v7ren/desktoplab) | 6e5360e | MIT, see `LICENSES/MIT-desktoplab.txt` | Crate renamed `hop-proto` to `desk-proto`. Workspace deps replaced with concrete versions (`postcard` 1 added). Header comment added to each file. No logic change. |

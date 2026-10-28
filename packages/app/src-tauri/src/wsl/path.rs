//! Windows path <-> Linux path mapping for Linux Inside (pure, no I/O).
//!
//! Only the types are here so far: `model.rs` (`LinuxPlace`) needs `Mounts`. The mapping
//! functions (`to_linux`, `to_windows`, `distro_path`) land with the path task (spec section 3.3).

/// Where the distro's drives and network share sit, as the probe found them.
#[derive(Debug, Clone, PartialEq)]
pub struct Mounts {
    pub distro: String,
    /// `/mnt/` (or `/` when `wsl.conf` sets `root=/`). Always ends with `/`.
    pub mount_root: String,
    /// `\\wsl.localhost\Ubuntu`.
    pub unc_root: String,
}

/// Why a path cannot be mapped. Mapping never normalises a bad path silently.
#[derive(Debug, PartialEq)]
pub enum PathError {
    Empty,
    Relative,
    NetworkShare,
    OtherDistro(String),
    InsideDistro,
    BadDrive,
    DotDot,
}

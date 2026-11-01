//! The loopback soak of My computers: two linked computers in one process on real sockets bound to 127.0.0.1, a pretend
//! PC underneath (no real pointer, keys, hooks or clipboard). Run by hand:
//!
//! ```text
//! cargo run --example desk_soak -- --secs 60 --copy-mb 8
//! ```
//!
//! Prints a report and exits 0 (PASS) or 1 (FAIL). See `src/desk_soak.rs` and `docs/features/my-computers-tests.md`.

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    let code = puppet_master_app_lib::desk_soak::run_cli(std::env::args().skip(1).collect()).await;
    std::process::exit(code);
}

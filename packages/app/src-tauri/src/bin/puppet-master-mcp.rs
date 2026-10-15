//! `puppet-master-mcp`: the stdio MCP shim binary (see `src/mcp_shim/mod.rs`).

// The shim's modules live in `src/mcp_shim/`, not `src/bin/puppet_master_mcp/`: the Tauri bundler
// treats every folder under `src/bin` as a binary and then fails on a missing `puppet_master_mcp.exe`.
#[path = "../mcp_shim/mod.rs"]
mod puppet_master_mcp;

fn main() {
    // Hook Lane emitter: decided before anything else runs (no logging, no MCP state).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hook") {
        puppet_master_mcp::hook::run(&args);
    }
    puppet_master_mcp::run();
}

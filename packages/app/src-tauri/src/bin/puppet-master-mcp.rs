//! `puppet-master-mcp`: the stdio MCP shim binary (see `puppet_master_mcp/mod.rs`).

mod puppet_master_mcp;

fn main() {
    // Hook Lane emitter: decided before anything else runs (no logging, no MCP state).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("hook") {
        puppet_master_mcp::hook::run(&args);
    }
    puppet_master_mcp::run();
}

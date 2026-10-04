//! `puppet-master-mcp`: the stdio MCP shim binary (see `puppet_master_mcp/mod.rs`).

mod puppet_master_mcp;

fn main() {
    puppet_master_mcp::run();
}

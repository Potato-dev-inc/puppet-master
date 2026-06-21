# @puppet-master/mcp

stdio MCP server for [Puppet Master](https://github.com/Potato-dev-inc/puppet-master). Proxies MCP tool calls to the local Puppet Master HTTP bridge.

## Prerequisites

1. Start Puppet Master (`npx puppet-master` or the desktop app).
2. The bridge port file must exist (written automatically when the app starts).

## Cursor / Claude Desktop

```json
{
  "mcpServers": {
    "puppet-master": {
      "command": "npx",
      "args": ["-y", "@puppet-master/mcp"]
    }
  }
}
```

## Codex CLI

```bash
codex mcp add puppet-master -- npx -y @puppet-master/mcp
```

## Tools

`bridge_health`, `list_panes`, `spawn_agent`, `read_terminal_buffer`, `write_terminal_input`, `kill_pane_process`, and related orchestration helpers.

See the [MCP_HOSTS.md](https://github.com/Potato-dev-inc/puppet-master/blob/main/MCP_HOSTS.md) guide in the main repo.

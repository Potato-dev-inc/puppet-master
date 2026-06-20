# Contributing

## Local Tool Config

Local agent and editor configuration is intentionally not tracked:

- `.claude/`
- `.codex/`
- `.cursor/`
- `.mcp.json`
- `opencode.json`

Use `.mcp.json.example` or the host-specific examples in `MCP_HOSTS.md` as a starting point, then keep your actual local paths and credentials outside git.

## Commits

Use scoped conventional commits for new work, for example:

```text
feat(mobile): add pairing retry state
fix(mcp): handle missing bridge port file
docs(readme): update smoke test commands
```

Do not rewrite published history just to clean up older commit messages.

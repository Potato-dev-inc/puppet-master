import { describe, expect, it } from 'vitest';
import {
  PUPPET_MASTER_MCP_COMMAND,
  PUPPET_MASTER_MCP_NAME,
  isOrchestratorMcpBackend,
} from './mcp-config';

describe('mcp-config', () => {
  it('defines the puppet-master MCP server identity', () => {
    expect(PUPPET_MASTER_MCP_NAME).toBe('puppet-master');
    expect(PUPPET_MASTER_MCP_COMMAND.command).toBe('npx');
    expect(PUPPET_MASTER_MCP_COMMAND.args).toContain('@puppet-master/mcp');
  });

  it('only treats CLI orchestrator backends as MCP install targets', () => {
    expect(isOrchestratorMcpBackend('api')).toBe(false);
    expect(isOrchestratorMcpBackend('opencode_cli')).toBe(true);
  });
});

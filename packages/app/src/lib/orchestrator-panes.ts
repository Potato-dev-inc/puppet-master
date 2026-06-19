import type { AgentType, OrchestratorBackend } from '@puppet-master/shared';
import { ORCHESTRATOR_PANE_PREFIX } from '@puppet-master/shared';
import { ensureOrchestratorMcp } from './mcp-config';
import { tauri, type PaneInfo } from './tauri';

export type CliOrchestratorBackend = Exclude<OrchestratorBackend, 'api'>;

export { ORCHESTRATOR_PANE_PREFIX };

export const ORCHESTRATOR_PANE_ID: Record<CliOrchestratorBackend, string> = {
  claude_cli: `${ORCHESTRATOR_PANE_PREFIX}claude`,
  codex_cli: `${ORCHESTRATOR_PANE_PREFIX}codex`,
  opencode_cli: `${ORCHESTRATOR_PANE_PREFIX}opencode`,
};

export const BACKEND_AGENT: Record<CliOrchestratorBackend, AgentType> = {
  claude_cli: 'claude',
  codex_cli: 'codex',
  opencode_cli: 'opencode',
};

export const BACKEND_LABEL: Record<CliOrchestratorBackend, string> = {
  claude_cli: 'Claude Code',
  codex_cli: 'Codex CLI',
  opencode_cli: 'OpenCode',
};

export function isCliOrchestratorBackend(
  backend: OrchestratorBackend,
): backend is CliOrchestratorBackend {
  return backend !== 'api';
}

export function isOrchestratorPaneId(paneId: string): boolean {
  return paneId.startsWith(ORCHESTRATOR_PANE_PREFIX);
}

export function orchestratorPaneIdForBackend(backend: CliOrchestratorBackend): string {
  return ORCHESTRATOR_PANE_ID[backend];
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export async function ensureOrchestratorPane(
  backend: CliOrchestratorBackend,
  cwd: string,
): Promise<string> {
  const mcp = await ensureOrchestratorMcp(backend, cwd);
  if (!mcp.installed) {
    throw new Error(mcp.message || 'Puppet Master MCP install incomplete');
  }

  const agent = BACKEND_AGENT[backend];
  const stableId = ORCHESTRATOR_PANE_ID[backend];
  const panes = await tauri.listPanes();
  const existing = panes.find((pane) => pane.id === stableId && pane.status !== 'error');
  if (existing?.cwd === cwd) return existing.id;
  if (existing) {
    await tauri.killPane(stableId);
  }

  const paneId = await tauri.spawnPane({
    agent_type: agent,
    pane_id: stableId,
    cwd,
    cols: 100,
    rows: 40,
  });
  await sleep(1200);
  return paneId;
}

export function findOrchestratorPane(
  panes: PaneInfo[],
  backend: CliOrchestratorBackend,
): PaneInfo | undefined {
  const stableId = ORCHESTRATOR_PANE_ID[backend];
  return panes.find((pane) => pane.id === stableId && pane.status !== 'error');
}

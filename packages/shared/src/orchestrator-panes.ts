import type { PaneInfo } from './protocol.js';

/** Stable id prefix for the dedicated CLI orchestrator pane (sidebar / mobile). */
export const ORCHESTRATOR_PANE_PREFIX = 'puppet-master-orchestrator-';

export function isOrchestratorPaneId(paneId: string): boolean {
  return paneId.startsWith(ORCHESTRATOR_PANE_PREFIX);
}

export function isWorkerPane(pane: Pick<PaneInfo, 'id'>): boolean {
  return !isOrchestratorPaneId(pane.id);
}

/** Prefer an open worker pane of the same agent (never the dedicated orchestrator pane). */
export function findReusableWorkerPane(panes: PaneInfo[], agentType: string): PaneInfo | undefined {
  const matches = panes.filter(
    (p) => isWorkerPane(p) && p.agent_type === agentType && p.status !== 'error',
  );
  if (matches.length === 0) return undefined;
  const ready = matches.find((p) => p.status === 'waiting_input' || p.status === 'idle');
  if (ready) return ready;
  return matches.sort((a, b) => b.created_at - a.created_at)[0];
}

/** Human-readable pane list for orchestrator MCP tools. */
export function formatPaneListForOrchestrator(panes: PaneInfo[]): string {
  if (panes.length === 0) return '(no panes)';
  return panes
    .map((p) => {
      if (isOrchestratorPaneId(p.id)) {
        return (
          `${p.id} | agent=${p.agent_type} | role=orchestrator | status=${p.status}` +
          ' | DO NOT write_terminal_input, kill, or delegate work here — spawn/use worker panes'
        );
      }
      const ready = p.status === 'waiting_input' || p.status === 'idle';
      return (
        `${p.id} | agent=${p.agent_type} | role=worker | status=${p.status} | cwd=${p.cwd}` +
        (ready ? ' ← ready for write_terminal_input' : '')
      );
    })
    .join('\n');
}

export function assertWorkerPaneTarget(paneId: string): void {
  if (isOrchestratorPaneId(paneId)) {
    throw new Error(
      `pane ${paneId} is the dedicated orchestrator — do not control it via MCP. ` +
        'Spawn or reuse a worker pane (id does not start with puppet-master-orchestrator-).',
    );
  }
}

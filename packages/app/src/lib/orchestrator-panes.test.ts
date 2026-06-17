import { describe, expect, it } from 'vitest';
import type { PaneInfo } from './tauri';
import {
  BACKEND_AGENT,
  ORCHESTRATOR_PANE_ID,
  findOrchestratorPane,
  isCliOrchestratorBackend,
  isOrchestratorPaneId,
  orchestratorPaneIdForBackend,
} from './orchestrator-panes';

function mockPane(id: string, agentType: string, createdAt: number): PaneInfo {
  return {
    id,
    agent_type: agentType,
    pid: 1,
    status: 'running',
    created_at: createdAt,
    last_output_at: null,
    cwd: '/tmp',
    cols: 80,
    rows: 24,
  };
}

describe('orchestrator-panes', () => {
  it('identifies orchestrator pane ids by prefix', () => {
    expect(isOrchestratorPaneId('puppet-master-orchestrator-opencode')).toBe(true);
    expect(isOrchestratorPaneId('worker-pane-1')).toBe(false);
  });

  it('maps cli backends to stable pane ids and agents', () => {
    expect(orchestratorPaneIdForBackend('opencode_cli')).toBe(
      'puppet-master-orchestrator-opencode',
    );
    expect(BACKEND_AGENT.opencode_cli).toBe('opencode');
  });

  it('distinguishes api vs cli orchestrator backends', () => {
    expect(isCliOrchestratorBackend('api')).toBe(false);
    expect(isCliOrchestratorBackend('opencode_cli')).toBe(true);
  });

  it('finds only the stable orchestrator pane for a backend', () => {
    const panes = [
      mockPane('random-opencode', 'opencode', 2),
      mockPane(ORCHESTRATOR_PANE_ID.opencode_cli, 'opencode', 1),
    ];

    expect(findOrchestratorPane(panes, 'opencode_cli')?.id).toBe(
      ORCHESTRATOR_PANE_ID.opencode_cli,
    );
  });

  it('requires matching cwd before reusing an orchestrator pane', () => {
    const stableId = ORCHESTRATOR_PANE_ID.opencode_cli;
    const panes = [mockPane(stableId, 'opencode', 1)];
    panes[0]!.cwd = '/old/project';

    expect(findOrchestratorPane(panes, 'opencode_cli')?.cwd).toBe('/old/project');
    expect(stableId.startsWith('puppet-master-orchestrator-')).toBe(true);
  });
});

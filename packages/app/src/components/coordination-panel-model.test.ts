import { describe, expect, it } from 'vitest';
import type { LockConflictProjection, LockProjection, SessionTimelineEvent, TaskProjection } from '../lib/bridge';
import {
  buildDelegationPreviewRequest,
  latestTimelineItems,
  lockConflictTitle,
  mcpHealthLabel,
  paneRoleLabel,
} from './coordination-panel-model';

describe('coordination-panel-model', () => {
  it('labels pane roles for UI display', () => {
    expect(paneRoleLabel('implementer')).toBe('implementer');
    expect(paneRoleLabel(null)).toBe('unassigned');
  });

  it('labels MCP health from bridge state and registry count', () => {
    expect(mcpHealthLabel(false, 0)).toBe('bridge offline');
    expect(mcpHealthLabel(true, 23)).toBe('23 registry tools');
  });

  it('returns newest timeline events first', () => {
    const timeline: SessionTimelineEvent[] = [
      { timestamp_ms: 1, actor: 'a', event_type: 'Old', summary: 'old' },
      { timestamp_ms: 2, actor: 'a', event_type: 'New', summary: 'new' },
    ];
    expect(latestTimelineItems(timeline, 1)).toEqual([timeline[1]]);
  });

  it('formats lock conflict state', () => {
    const conflict: LockConflictProjection = {
      resource_id: 'file:README.md',
      requested_owner_id: 'agent-b',
      existing_owner_id: 'agent-a',
      timestamp_ms: 1,
    };
    expect(lockConflictTitle(conflict)).toBe('file:README.md blocked agent-b');
  });

  it('builds a delegation preview request from task and locks', () => {
    const task: TaskProjection = {
      id: 'task-1',
      title: 'Finish migration',
      status: 'open',
      exclusive: true,
      claimed_by: null,
      lease_expires_at_ms: null,
      reviewer_id: null,
      evidence: null,
      blocked_reason: null,
    };
    const locks: LockProjection[] = [
      {
        resource_id: 'file:rust-migration-plan.md',
        resource_type: 'file',
        owner: 'agent-a',
        lease_expires_at_ms: null,
      },
    ];
    const request = buildDelegationPreviewRequest(task, locks);
    expect(request.intent).toBe('Finish migration');
    expect(request.task_id).toBe('task-1');
    expect(request.locked_resources).toEqual(['file:rust-migration-plan.md']);
    expect(request.acceptance_criteria.length).toBeGreaterThan(0);
  });
});

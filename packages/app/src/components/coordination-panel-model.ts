import type {
  DelegateTaskRequest,
  LockConflictProjection,
  LockProjection,
  PaneRole,
  SessionTimelineEvent,
  TaskProjection,
} from '../lib/bridge';

export function paneRoleLabel(role: PaneRole | null | undefined): string {
  return role ?? 'unassigned';
}

export function mcpHealthLabel(bridgeReady: boolean, toolCount: number): string {
  if (!bridgeReady) return 'bridge offline';
  return `${toolCount} registry tools`;
}

export function latestTimelineItems(
  timeline: SessionTimelineEvent[],
  limit = 8,
): SessionTimelineEvent[] {
  return [...timeline].slice(-limit).reverse();
}

export function lockConflictTitle(conflict: LockConflictProjection): string {
  return `${conflict.resource_id} blocked ${conflict.requested_owner_id}`;
}

export function buildDelegationPreviewRequest(
  task: TaskProjection | undefined,
  locks: LockProjection[],
  currentGoal?: string | null,
): DelegateTaskRequest {
  const intent = task?.title || currentGoal || 'Coordinate the selected Puppet Master task';
  return {
    task_id: task?.id,
    intent,
    acceptance_criteria: [
      'Worker reports the concrete changes made',
      'Worker reports verification commands and outcomes',
    ],
    locked_resources: locks.map((lock) => lock.resource_id),
    evidence_required: ['Files changed', 'Tests or checks run', 'Known gaps'],
    token_budget_hint: 8000,
    timeout_ms: 600_000,
  };
}

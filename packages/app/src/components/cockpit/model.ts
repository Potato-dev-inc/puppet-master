// Pure helpers for the cockpit: grouping, labels, elapsed text, palette matching.
import type { PaneData } from '../../hooks/usePaneRegistry';
import type { SpawnStageEvent } from '../../lib/cockpit-api';

export type SpinnerStatus = 'running' | 'waiting_input' | 'idle' | 'error' | 'exited';

export function spinnerStatus(status: string): SpinnerStatus {
  switch (status) {
    case 'running':
    case 'waiting_input':
    case 'idle':
    case 'error':
      return status;
    default:
      return 'exited';
  }
}

export function stateText(status: string): string {
  switch (status) {
    case 'running':
      return 'Working';
    case 'waiting_input':
      return 'Needs you';
    case 'idle':
      return 'Idle';
    case 'error':
      return 'Error';
    default:
      return 'Exited';
  }
}

export interface PaneGroups {
  needs: PaneData[];
  working: PaneData[];
  idle: PaneData[];
}

/** Needs you = waiting_input + error, Working = running, Idle = idle (and anything else). */
export function groupPanes(list: PaneData[], questionPanes?: ReadonlySet<string>): PaneGroups {
  const groups: PaneGroups = { needs: [], working: [], idle: [] };
  for (const pane of list) {
    if (pane.status === 'waiting_input' || pane.status === 'error' || questionPanes?.has(pane.info.id)) {
      groups.needs.push(pane);
    } else if (pane.status === 'running') groups.working.push(pane);
    else groups.idle.push(pane);
  }
  return groups;
}

/** "42s", "5m", "3h 12m". */
export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  const rem = m % 60;
  return rem ? `${h}h ${rem}m` : `${h}h`;
}

/** Display name per pane: the agent label, numbered when several panes run the same agent. */
export function paneNames(list: PaneData[], label: (agentId: string) => string): Map<string, string> {
  const totals = new Map<string, number>();
  for (const p of list) totals.set(p.info.agent_type, (totals.get(p.info.agent_type) ?? 0) + 1);
  const seen = new Map<string, number>();
  const names = new Map<string, string>();
  for (const p of list) {
    const id = p.info.agent_type;
    const n = (seen.get(id) ?? 0) + 1;
    seen.set(id, n);
    names.set(p.info.id, (totals.get(id) ?? 0) > 1 ? `${label(id)} ${n}` : label(id));
  }
  return names;
}

export function spawnLogLine(e: SpawnStageEvent, agentName?: string): string {
  if (e.message) return e.message;
  switch (e.stage) {
    case 'claiming':
      return 'Getting a ready agent';
    case 'spawning':
      return agentName ? `Starting ${agentName}` : 'Starting the agent';
    case 'attached':
      return 'Connected';
    case 'ready':
      return 'Ready';
    case 'failed':
      return 'Could not start';
  }
}

/** Every query word must appear in the haystack (case-insensitive). */
export function matchesQuery(haystack: string, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const text = haystack.toLowerCase();
  return words.every((w) => text.includes(w));
}

/** Short one-line summary of a prompt for rows. */
export function promptSummary(prompt: { summary?: string; text: string } | null | undefined): string {
  if (!prompt) return '';
  const raw = (prompt.summary || prompt.text || '').replace(/\s+/g, ' ').trim();
  return raw.length > 140 ? `${raw.slice(0, 139)}…` : raw;
}

/** Flatten a receipt JSON value into display rows (dot-joined keys, scalars stringified). */
export function receiptRows(value: unknown, prefix = ''): { key: string; value: string }[] {
  if (value === null || value === undefined) return prefix ? [{ key: prefix, value: '—' }] : [];
  if (Array.isArray(value)) {
    if (value.length === 0) return [{ key: prefix, value: '—' }];
    if (value.every((v) => typeof v !== 'object' || v === null)) {
      return [{ key: prefix, value: value.map(String).join(', ') }];
    }
    return value.flatMap((v, i) => receiptRows(v, `${prefix}[${i}]`));
  }
  if (typeof value === 'object') {
    const entries = Object.entries(value as Record<string, unknown>);
    if (entries.length === 0) return prefix ? [{ key: prefix, value: '—' }] : [];
    return entries.flatMap(([k, v]) => receiptRows(v, prefix ? `${prefix}.${k}` : k));
  }
  return [{ key: prefix, value: String(value) }];
}

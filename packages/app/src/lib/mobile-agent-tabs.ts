// Pure helpers for the mobile agent tabs (PwaApp.tsx): tab order and labels, status display and the
// short "what is it doing" preview shown on the All overview.
import type { PaneInfo } from '@puppet-master/shared';
import type { AgentRegistryListing, LaunchEntry } from './agentRegistry';

export const ALL_TAB = 'all';

export interface StatusMeta {
  label: string;
  dot: string;
  /** The agent needs the user (waiting at a prompt) or failed. */
  attention: boolean;
  pulse: boolean;
}

const STATUS: Record<string, StatusMeta> = {
  running: { label: 'working', dot: 'bg-pm-ok', attention: false, pulse: true },
  waiting_input: { label: 'needs you', dot: 'bg-pm-warn', attention: true, pulse: false },
  error: { label: 'error', dot: 'bg-pm-err', attention: true, pulse: false },
  idle: { label: 'idle', dot: 'bg-pm-muted', attention: false, pulse: false },
};

export function statusMeta(status: string): StatusMeta {
  return STATUS[status] ?? { label: status.replace(/_/g, ' '), dot: 'bg-pm-muted', attention: false, pulse: false };
}

/** Oldest first, like terminal tabs opening left to right. */
export function orderPanes(panes: PaneInfo[]): PaneInfo[] {
  return [...panes].sort((a, b) => a.created_at - b.created_at || a.id.localeCompare(b.id));
}

/** Last folder of a path (`C:\a\b\` -> `b`). */
export function projectName(cwd: string): string {
  const parts = cwd.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? cwd;
}

/** Tab title: the agent id, numbered when several panes run the same agent (`claude 2`). */
export function tabLabels(panes: PaneInfo[]): Map<string, string> {
  const ordered = orderPanes(panes);
  const totals = new Map<string, number>();
  for (const pane of ordered) totals.set(pane.agent_type, (totals.get(pane.agent_type) ?? 0) + 1);
  const seen = new Map<string, number>();
  const labels = new Map<string, string>();
  for (const pane of ordered) {
    const n = (seen.get(pane.agent_type) ?? 0) + 1;
    seen.set(pane.agent_type, n);
    labels.set(pane.id, (totals.get(pane.agent_type) ?? 0) > 1 ? `${pane.agent_type} ${n}` : pane.agent_type);
  }
  return labels;
}

/** "just now", "42s ago", "5m ago", "3h ago"; null when the pane never printed anything. */
export function ago(at: number | null, now: number): string | null {
  if (at === null) return null;
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 5) return 'just now';
  if (s < 60) return `${s}s ago`;
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  return `${Math.floor(s / 3600)}h ago`;
}

const FRAME_CHARS = /^[\s┃│║╹╻▀▄█▌▐─━═╭╮╰╯┌┐└┘├┤┬┴┼>›❯•·]+/u;

/** Lines at the bottom of a screen that may be TUI chrome (input box, footer) rather than output. */
const CHROME_WINDOW = 8;
const CHROME_MAX_LINES = 3;

function meaningful(line: string): boolean {
  return (line.match(/[\p{L}\p{N}]/gu)?.length ?? 0) >= 3;
}

/**
 * The last `max` meaningful lines of a pane's screen text: frame/box characters stripped, lines
 * with almost no letters or digits (borders, spinners, blank space) dropped, and the TUI's bottom
 * chrome skipped: up to 3 meaningful lines at the very bottom that sit under a gap of 2+ empty
 * lines (an input box and its footer), when there is output above that gap.
 */
export function previewLines(screen: string, max = 2): string[] {
  const lines = screen.split(/\r?\n/).map((raw) => raw.replace(FRAME_CHARS, '').replace(/\s+/g, ' ').trim());
  let end = lines.length;
  while (end > 0 && !meaningful(lines[end - 1])) end--;
  let tail = 0;
  for (let i = end - 1; i > 0 && i >= end - CHROME_WINDOW && tail <= CHROME_MAX_LINES; i--) {
    if (meaningful(lines[i])) {
      tail++;
    } else if (!meaningful(lines[i - 1])) {
      if (lines.slice(0, i - 1).some(meaningful)) end = i - 1;
      break;
    }
  }
  return lines.slice(0, end).filter(meaningful).slice(-max);
}

/**
 * What the phone's "new agent" sheet offers: every registered agent, those that can open a visible
 * pane first; headless-only and not-installed agents are listed disabled with the reason.
 */
export function phoneLaunchEntries(listing: AgentRegistryListing): LaunchEntry[] {
  const entries = listing.agents.map((agent): LaunchEntry => {
    const entry: LaunchEntry = { type: agent.id, label: agent.label || agent.id, icon: agent.icon };
    if (agent.experimental) entry.experimental = true;
    if (!agent.modes.visible) {
      entry.disabled = true;
      entry.disabledReason = 'Headless only';
    } else if (!agent.installed) {
      entry.disabled = true;
      entry.disabledReason = 'Not installed';
    }
    return entry;
  });
  return [...entries.filter((e) => !e.disabled), ...entries.filter((e) => e.disabled)];
}

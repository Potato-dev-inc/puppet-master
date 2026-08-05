import type { PaneInfo as BridgePaneInfo } from '@puppet-master/shared';
import type { PaneInfo as TauriPaneInfo } from './tauri';

type PaneStateEstimate = TauriPaneInfo['pane_state'] | BridgePaneInfo['pane_state'];

export const PANE_STATE_COLOR: Record<string, string> = {
  thinking: 'bg-pm-accent',
  streaming_answer: 'bg-pm-accent',
  editing: 'bg-pm-ok',
  running_command: 'bg-pm-ok',
  test_running: 'bg-pm-ok',
  awaiting_permission: 'bg-pm-warn',
  awaiting_user_answer: 'bg-pm-warn',
  menu_navigation: 'bg-pm-warn',
  test_failed: 'bg-pm-err',
  rate_limited: 'bg-pm-err',
  auth_required: 'bg-pm-err',
  stuck: 'bg-pm-err',
  test_passed: 'bg-pm-ok',
  complete_claimed: 'bg-pm-ok',
  needs_review: 'bg-pm-warn',
  idle: 'bg-pm-muted',
  exited: 'bg-pm-muted',
};

export function paneStateColor(state: PaneStateEstimate | undefined, fallbackStatus: string): string {
  if (state) return PANE_STATE_COLOR[state.state] ?? 'bg-pm-muted';
  switch (fallbackStatus) {
    case 'running':
      return 'bg-pm-ok';
    case 'waiting_input':
      return 'bg-pm-warn';
    case 'error':
      return 'bg-pm-err';
    default:
      return 'bg-pm-muted';
  }
}

export function paneStateLabel(state: PaneStateEstimate | undefined, fallbackStatus: string): string {
  if (!state) return fallbackStatus.replace(/_/g, ' ');
  return state.summary || state.state.replace(/_/g, ' ');
}

export function paneHeaderTitle(
  agentLabel: string,
  state: PaneStateEstimate | undefined,
  fallbackStatus: string,
): string {
  return `${agentLabel}: ${paneStateLabel(state, fallbackStatus)}`;
}

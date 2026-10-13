// Mirrors Rust `pane_windows::close_decision`: a pane that is working asks before it is closed.
import { loadSettings } from './settings';

export const CONFIRM_CLOSE_EVENT = 'pane-window://confirm-close';

export function isBusyStatus(status: string | undefined): boolean {
  return status === 'running' || status === 'waiting_input';
}

/** True when closing a pane in this state must ask first. `setting` is `confirm_close_running_pane`. */
export function needsCloseConfirm(status: string | undefined, setting: boolean | undefined): boolean {
  return (setting ?? true) && isBusyStatus(status);
}

export async function confirmCloseEnabled(): Promise<boolean> {
  try {
    return (await loadSettings()).confirm_close_running_pane ?? true;
  } catch {
    return true;
  }
}

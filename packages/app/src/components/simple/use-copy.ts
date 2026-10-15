// The copy a pane works in: the git worktree behind "Working on a copy". Looked up through the same
// `pane_worktree` command the Changes tab uses; `loaded` tells "not looked up yet" from "none".
import { useEffect, useState } from 'react';
import { cockpitApi, type WorktreeItem } from '../../lib/cockpit-api';

export interface CopyState {
  item: WorktreeItem | null;
  loaded: boolean;
}

export function useCopy(
  paneId: string,
  status: string,
  api: Pick<typeof cockpitApi, 'paneWorktree'> = cockpitApi,
): CopyState {
  const [state, setState] = useState<CopyState>({ item: null, loaded: false });
  useEffect(() => {
    setState({ item: null, loaded: false });
  }, [paneId]);
  useEffect(() => {
    if (!paneId) {
      setState({ item: null, loaded: true });
      return;
    }
    let cancelled = false;
    api
      .paneWorktree(paneId)
      .then((found) => !cancelled && setState({ item: found && found.exists ? found : null, loaded: true }))
      .catch(() => !cancelled && setState({ item: null, loaded: true }));
    return () => {
      cancelled = true;
    };
  }, [api, paneId, status]);
  return state;
}

/** The project's name: the folder the copy came from. */
export function projectLabel(item: WorktreeItem | null, fallback: string): string {
  if (!item) return fallback;
  const parts = item.repo.replace(/[\\/]+$/, '').split(/[\\/]/);
  return parts[parts.length - 1] || fallback;
}

/** Whether a pane is on a copy. Simple mode only shows such panes: a copy is the one way it works. */
export function isOnCopy(item: WorktreeItem | null | undefined): boolean {
  return !!item && item.exists;
}

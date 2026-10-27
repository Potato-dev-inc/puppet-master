// The git branch of the folder a pane works in (null when it is not a git folder). Looked up again whenever the
// pane's status changes, which is when the agent may have switched branch.
import { useEffect, useState } from 'react';
import { cockpitApi } from '../../../lib/cockpit-api';

export function usePaneBranch(
  paneId: string,
  status: string,
  enabled: boolean,
  api: Pick<typeof cockpitApi, 'paneGitBranch'> = cockpitApi,
): string | null {
  const [branch, setBranch] = useState<string | null>(null);

  useEffect(() => {
    setBranch(null);
  }, [paneId]);

  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    api
      .paneGitBranch(paneId)
      .then((found) => {
        if (!cancelled) setBranch(found || null);
      })
      .catch(() => {
        // Not in the desktop shell, or the lookup failed: no branch line.
        if (!cancelled) setBranch(null);
      });
    return () => {
      cancelled = true;
    };
  }, [api, paneId, status, enabled]);

  return branch;
}

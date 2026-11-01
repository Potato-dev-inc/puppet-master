// The state of one chat session over ACP (phase, "stopping"): read once when the pane opens and kept fresh by the
// `acp-chat-state` event the backend sends at every change. `null` while unknown, or for a pane that is not one.
import { useEffect, useState } from 'react';
import { tauriAcpChatApi, type AcpChatApi, type AcpChatState } from '../lib/acpChat';

export function useAcpChatState(paneId: string, enabled: boolean, api: AcpChatApi = tauriAcpChatApi): AcpChatState | null {
  const [state, setState] = useState<AcpChatState | null>(null);

  useEffect(() => {
    if (!enabled) {
      setState(null);
      return;
    }
    let live = true;
    let off: (() => void) | null = null;
    setState(null);
    // Listen before reading, so a change that happens while the read is in flight is not lost.
    void api
      .watch((id, next) => {
        if (live && id === paneId) setState(next);
      })
      .then((stop) => {
        if (live) off = stop;
        else stop();
      })
      .catch(() => undefined);
    api
      .state(paneId)
      .then((first) => {
        // An event that came first is newer than this read.
        if (live) setState((prev) => prev ?? first);
      })
      .catch(() => undefined);
    return () => {
      live = false;
      off?.();
    };
  }, [paneId, enabled, api]);

  return state;
}

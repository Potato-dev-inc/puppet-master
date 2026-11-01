// The state of a chat session over ACP as the phone sees it: `GET /acp/sessions/{pane}` every 2 s while the page is
// visible (the PC pushes no event to a phone for it). `null` while unknown, for a PC without the route, and when this
// phone may not read it.
import { useEffect, useState } from 'react';
import type { AcpChatState } from '../lib/acpChat';
import type { BridgeClient } from '../lib/bridge-types';

export const ACP_STATE_POLL_MS = 2000;

export function useAcpPhoneState(bridge: Pick<BridgeClient, 'acpSession'> | null, paneId: string, enabled: boolean): AcpChatState | null {
  const [state, setState] = useState<AcpChatState | null>(null);
  useEffect(() => {
    setState(null);
    if (!enabled || !bridge?.acpSession) return;
    let live = true;
    let timer = 0;
    const ask = bridge.acpSession.bind(bridge);
    const tick = () => {
      if (typeof document !== 'undefined' && document.visibilityState === 'hidden') {
        timer = window.setTimeout(tick, ACP_STATE_POLL_MS);
        return;
      }
      ask(paneId)
        .then((next) => live && setState(next))
        // A dropped answer is tried again; the last state stays on screen meanwhile.
        .catch(() => undefined)
        .finally(() => {
          if (live) timer = window.setTimeout(tick, ACP_STATE_POLL_MS);
        });
    };
    tick();
    return () => {
      live = false;
      window.clearTimeout(timer);
    };
  }, [bridge, paneId, enabled]);
  return state;
}

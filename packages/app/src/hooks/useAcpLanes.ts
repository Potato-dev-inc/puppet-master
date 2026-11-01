// The agents that can run as a chat (ACP) and each one's setting, read from the backend whenever `active` turns on (the "+"
// menu opens). `null` while unknown and without a desktop shell. The Labs switch is part of the answer.
import { useEffect, useState } from 'react';
import { tauriAcpChatApi, type AcpChatApi, type AcpLaneView } from '../lib/acpChat';

export function useAcpLanes(active: boolean, api: Pick<AcpChatApi, 'laneGet'> = tauriAcpChatApi): AcpLaneView | null {
  const [view, setView] = useState<AcpLaneView | null>(null);
  useEffect(() => {
    if (!active) return;
    let live = true;
    api
      .laneGet()
      .then((v) => live && setView(v))
      .catch(() => live && setView(null));
    return () => {
      live = false;
    };
  }, [active, api]);
  return view;
}

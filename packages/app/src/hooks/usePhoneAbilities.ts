// What this phone may do, read from the PC (`GET /pair/me`) when it connects, when the stream
// resyncs and when the app comes back to the front (the person can change it on the PC at any time).
import { useCallback, useEffect, useState } from 'react';
import type { BridgeClient } from '../lib/bridge-types';
import { abilitiesFrom, ALL_ABILITIES, type PhoneAbilities } from '../lib/phone-abilities';

export function usePhoneAbilities(bridge: BridgeClient | null, resyncKey: number): PhoneAbilities {
  const [abilities, setAbilities] = useState<PhoneAbilities>(ALL_ABILITIES);

  const read = useCallback(() => {
    if (!bridge?.pairMe) return;
    bridge
      .pairMe()
      .then((me) => setAbilities(abilitiesFrom(me)))
      // An old PC without the route, or a dropped link: keep what is shown (the PC refuses typing anyway).
      .catch(() => undefined);
  }, [bridge]);

  useEffect(() => {
    read();
    const onVisible = () => {
      if (document.visibilityState === 'visible') read();
    };
    document.addEventListener('visibilitychange', onVisible);
    return () => document.removeEventListener('visibilitychange', onVisible);
  }, [read, resyncKey]);

  return abilities;
}

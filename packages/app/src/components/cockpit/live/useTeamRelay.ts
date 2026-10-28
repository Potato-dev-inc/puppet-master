// The team relay address, when a team relay is set. The Live Room relay setting falls back to it. '' while unknown.
import { useEffect, useState } from 'react';
import { teamApi } from '../../../lib/team';

export function useTeamRelay(enabled = true): string {
  const [relay, setRelay] = useState('');
  useEffect(() => {
    if (!enabled) return;
    let live = true;
    teamApi
      .configGet()
      .then((c) => live && setRelay(c.relay ?? ''))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [enabled]);
  return relay;
}

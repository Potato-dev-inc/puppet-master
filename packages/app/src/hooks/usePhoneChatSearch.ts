// The hits for what is typed on the phone's search screen: asks the PC a moment after the last key, and drops an
// answer that arrives after a newer question (the same rule as the desktop palette's `useChatSearch`).
import { useEffect, useState } from 'react';
import type { BridgeClient } from '../lib/bridge';
import type { ChatHit } from '../lib/chatSearch';

/** `off`: the person turned the search index off on the PC. */
export type PhoneSearchState = 'idle' | 'searching' | 'done' | 'failed' | 'off';

export interface PhoneSearch {
  hits: ChatHit[];
  state: PhoneSearchState;
}

export const PHONE_SEARCH_LIMIT = 30;
const DEBOUNCE_MS = 150;

export function usePhoneChatSearch(bridge: BridgeClient, query: string): PhoneSearch {
  const [result, setResult] = useState<PhoneSearch>({ hits: [], state: 'idle' });

  useEffect(() => {
    const typed = query.trim();
    if (!typed) {
      setResult({ hits: [], state: 'idle' });
      return;
    }
    let live = true;
    setResult((prev) => ({ hits: prev.hits, state: 'searching' }));
    const timer = setTimeout(() => {
      const ask = bridge.searchChats ? bridge.searchChats(typed, PHONE_SEARCH_LIMIT) : Promise.reject(new Error('no search'));
      ask
        .then((answer) => {
          if (live) setResult(answer.enabled ? { hits: answer.hits, state: 'done' } : { hits: [], state: 'off' });
        })
        .catch(() => {
          if (live) setResult({ hits: [], state: 'failed' });
        });
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [bridge, query]);

  return result;
}

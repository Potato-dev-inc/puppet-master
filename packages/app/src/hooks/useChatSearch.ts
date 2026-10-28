// The hits for what is typed in the palette's "Search all chats" mode: asks the backend a moment after the last
// key, and drops an answer that arrives after a newer question.
import { useEffect, useState } from 'react';
import { searchChats, type ChatHit } from '../lib/chatSearch';

export type ChatSearchState = 'idle' | 'searching' | 'done' | 'failed';

export interface ChatSearch {
  hits: ChatHit[];
  state: ChatSearchState;
}

const DEBOUNCE_MS = 120;

export function useChatSearch(query: string, enabled: boolean): ChatSearch {
  const [result, setResult] = useState<ChatSearch>({ hits: [], state: 'idle' });

  useEffect(() => {
    const typed = query.trim();
    if (!enabled || !typed) {
      setResult({ hits: [], state: 'idle' });
      return;
    }
    let live = true;
    setResult((prev) => ({ hits: prev.hits, state: 'searching' }));
    const timer = setTimeout(() => {
      searchChats(typed)
        .then((hits) => live && setResult({ hits, state: 'done' }))
        .catch(() => live && setResult({ hits: [], state: 'failed' }));
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, enabled]);

  return result;
}

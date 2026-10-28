// The hits for what is typed in the palette's "Search all chats" mode: asks the backend a moment after the last
// key (or chip), and drops an answer that arrives after a newer question.
import { useEffect, useState } from 'react';
import { searchChats, type ChatHit } from '../lib/chatSearch';
import { hasChatFilters, searchFilters, type ChatFilters } from '../lib/chatFilters';

/** `off`: the person turned the search index off (Settings > General). */
export type ChatSearchState = 'idle' | 'searching' | 'done' | 'failed' | 'off';

export interface ChatSearch {
  hits: ChatHit[];
  state: ChatSearchState;
}

const DEBOUNCE_MS = 120;

/** `query`: the words typed (shortcuts already taken out); `filters`: the chips and shortcuts together. Words or a
 * filter are enough to search; with a filter and no words the newest items that pass it are listed. */
export function useChatSearch(query: string, filters: ChatFilters, enabled: boolean): ChatSearch {
  const [result, setResult] = useState<ChatSearch>({ hits: [], state: 'idle' });
  // The effect follows the filters' content, not their object: a new object with the same filters is not a new question.
  const filterKey = JSON.stringify(filters);

  useEffect(() => {
    const chosen: ChatFilters = JSON.parse(filterKey);
    const typed = query.trim();
    if (!enabled || (!typed && !hasChatFilters(chosen))) {
      setResult({ hits: [], state: 'idle' });
      return;
    }
    let live = true;
    setResult((prev) => ({ hits: prev.hits, state: 'searching' }));
    const timer = setTimeout(() => {
      searchChats(typed, undefined, searchFilters(chosen, Date.now()))
        .then((answer) => live && setResult(answer.enabled ? { hits: answer.hits, state: 'done' } : { hits: [], state: 'off' }))
        .catch(() => live && setResult({ hits: [], state: 'failed' }));
    }, DEBOUNCE_MS);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [query, filterKey, enabled]);

  return result;
}

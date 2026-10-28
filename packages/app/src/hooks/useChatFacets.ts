// The agents and folders the index holds, for the filter menus of "Search all chats". Loaded when the search opens.
import { useEffect, useState } from 'react';
import { chatSearchFacets, type ChatSearchFacets } from '../lib/chatSearch';

const NONE: ChatSearchFacets = { agents: [], folders: [] };

export function useChatFacets(enabled: boolean): ChatSearchFacets {
  const [facets, setFacets] = useState<ChatSearchFacets>(NONE);

  useEffect(() => {
    if (!enabled) return;
    let live = true;
    chatSearchFacets()
      .then((next) => live && setFacets(next))
      .catch(() => live && setFacets(NONE));
    return () => {
      live = false;
    };
  }, [enabled]);

  return facets;
}

import { createContext, useContext } from 'react';
import type { TurnSummary } from '../../lib/cockpit-api';

/** Latest turn summary per pane id, provided by CockpitApp. */
export const LastTurnContext = createContext<ReadonlyMap<string, TurnSummary>>(new Map());

export function useLastTurn(paneId: string): TurnSummary | undefined {
  return useContext(LastTurnContext).get(paneId);
}

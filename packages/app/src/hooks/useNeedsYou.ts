// The cockpit's single "Needs you" list (see lib/attention.ts). CockpitApp derives it once, with
// the Ask-bar notices included, and shares it through context; a component rendered without that
// provider (a test, a storybook) derives the same list from the cockpit state alone.
import { createContext, useContext, useEffect, useMemo } from 'react';
import { attentionMismatch, deriveNeedsYou, type NeedsYouItem } from '../lib/attention';
import { useAttention } from './useAttention';
import { tauriAskbarApi, useAskNotices, type AskbarApi } from '../lib/askbar';
import { useLoopGuard } from '../lib/loop-guard';
import type { CockpitApi } from './useCockpit';

export const NeedsYouContext = createContext<NeedsYouItem[] | null>(null);

type CockpitState = Pick<CockpitApi, 'registry' | 'prompts' | 'asked' | 'statusSince'>;

function derive(c: CockpitState, stuckPaneIds: string[], notices: { id: string; at_ms?: number }[]): NeedsYouItem[] {
  return deriveNeedsYou({
    panes: c.registry.paneList.map((p) => ({ id: p.info.id, status: p.status })),
    prompts: c.prompts,
    questions: c.asked.questions,
    stuckPaneIds,
    notices,
    statusSince: c.statusSince,
  });
}

/** For CockpitApp: derive once and provide. Includes the Ask-bar answers nobody saw yet. */
export function useNeedsYouSource(c: CockpitState, signals: Record<string, unknown>, askbar: AskbarApi = tauriAskbarApi): NeedsYouItem[] {
  const { notices } = useAskNotices(askbar);
  const items = useMemo(() => derive(c, Object.keys(signals), notices), [c.registry.paneList, c.prompts, c.asked.questions, c.statusSince, signals, notices]); // eslint-disable-line react-hooks/exhaustive-deps
  useShadowCheck(items);
  return items;
}

/**
 * Dev builds only: the Rust attention snapshot (src-tauri/src/attention) runs next to this
 * derivation, and a disagreement that lasts more than a second is a console warning. Nothing is
 * rendered from the snapshot here; this is how a drift between the two implementations is noticed.
 */
function useShadowCheck(items: NeedsYouItem[]): void {
  const snapshot = useAttention();
  useEffect(() => {
    if (!import.meta.env.DEV || !snapshot) return;
    if (attentionMismatch(items, snapshot).length === 0) return;
    const timer = window.setTimeout(() => {
      const diff = attentionMismatch(items, snapshot);
      if (diff.length > 0) console.warn('[attention] front-end and Rust lists disagree:', diff);
    }, 1000);
    return () => window.clearTimeout(timer);
  }, [items, snapshot]);
}

/** For any cockpit surface: the shared list, or the same derivation without Ask-bar notices. */
export function useNeedsYou(c: CockpitState): NeedsYouItem[] {
  const shared = useContext(NeedsYouContext);
  const { signals } = useLoopGuard();
  const local = useMemo(() => derive(c, Object.keys(signals), []), [c.registry.paneList, c.prompts, c.asked.questions, c.statusSince, signals]); // eslint-disable-line react-hooks/exhaustive-deps
  return shared ?? local;
}

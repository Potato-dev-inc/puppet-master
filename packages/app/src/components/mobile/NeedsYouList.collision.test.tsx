// @vitest-environment jsdom
// Collision Alert on the phone (SPEC-n33 3.9 item 4): the PC raises one plain `review` item in the Ready lane for a
// clash, with a title, a sentence and no buttons. The Needs you queue is the Blocking lane only, so such an item is
// never drawn there and never counted. This test pins that: a Ready-lane item neither adds a card nor adds to the count.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ITEM_ERROR } from '../../dev/attention-fixtures';
import type { AttentionItem, AttentionSnapshot } from '../../lib/attention';
import type { BridgeClient } from '../../lib/bridge-types';
import { queueCount } from '../../lib/needs-you-queue';
import { resetTouchForTests } from '../../hooks/useHeldItems';
import { NeedsYouList } from './NeedsYouList';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  resetTouchForTests();
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

/** What `attention::raise` makes of the Collision Alert item: kind review, Ready lane, no pane, no actions. */
const COLLISION_ITEM: AttentionItem = {
  id: 'Col1s10n',
  key: 'collision:ca-0123456789ab',
  kind: 'review',
  lane: 'ready',
  rank: 10,
  state: 'open',
  since_ms: Date.now() - 60_000,
  expires_at_ms: null,
  pane: null,
  title: 'Claude Code and Codex both changed src/auth.ts',
  sentence: 'Lines 40-80 overlap. Finish Claude Code first. It has stopped working.',
  risk: null,
  prompt: null,
  actions: [],
  resolution: null,
};

const snap = (items: AttentionItem[]): AttentionSnapshot => ({
  v: 1,
  rev: 1,
  counts: { blocking: items.filter((i) => i.lane === 'blocking' && i.state === 'open').length, ready: items.filter((i) => i.lane === 'ready').length },
  items,
});

const bridge = { answerAttention: vi.fn(), answerQuestion: vi.fn(), dismissQuestion: vi.fn() } as unknown as BridgeClient;

async function render(s: AttentionSnapshot, showEmpty = false) {
  await act(async () => {
    root.render(
      <NeedsYouList
        snapshot={s}
        bridge={bridge}
        canAnswer
        questions={[]}
        refreshQuestions={() => undefined}
        labels={new Map()}
        onOpenPane={() => undefined}
        showEmpty={showEmpty}
      />,
    );
  });
}

describe('a Collision Alert item (Ready lane, kind review, no actions)', () => {
  it('has no buttons, no pane and is not counted', () => {
    expect(COLLISION_ITEM.actions).toEqual([]);
    expect(COLLISION_ITEM.pane).toBeNull();
    expect(queueCount(snap([COLLISION_ITEM]))).toBe(0);
  });

  it('does not make a card and does not hide the empty state', async () => {
    await render(snap([COLLISION_ITEM]), true);
    expect(host.querySelector('[data-card-state]')).toBeNull();
    expect(host.textContent).toContain('Nothing needs you');
    expect(host.textContent).not.toContain('src/auth.ts');
  });

  it('does not change what the Needs you queue draws or counts', async () => {
    const withItem = snap([ITEM_ERROR, COLLISION_ITEM]);
    await render(withItem);
    const cards = host.querySelectorAll('[data-card-state]');
    expect(cards).toHaveLength(1);
    expect(cards[0].getAttribute('data-item-kind')).toBe('error');
    expect(queueCount(withItem)).toBe(queueCount(snap([ITEM_ERROR])));
    expect(host.querySelector('button')?.textContent ?? '').not.toMatch(/ignore|pause|stop/i);
  });
});

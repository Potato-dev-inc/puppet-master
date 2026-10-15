// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Digest, TurnSummary } from '../../lib/cockpit-api';
import { useAway } from '../../hooks/useAway';
import { LastTurn } from './LastTurn';
import { LastTurnContext, useLastTurn } from './away-context';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_800_000_000_000;

function summary(extra: Partial<TurnSummary> = {}): TurnSummary {
  return {
    paneId: 'p1',
    agent: 'Claude Code',
    turn: 3,
    startedAtMs: NOW - 200_000,
    endedAtMs: NOW - 60_000,
    durationMs: 140_000,
    files: { count: 4, added: 86, deleted: 12, paths: ['src/a.ts', 'src/b.ts'] },
    approvals: [],
    end: { kind: 'waiting_input', prompt: 'allow `npm test`?' },
    sentence: 'Claude Code edited 4 files (+86 −12), you approved 1 command, now waiting: allow `npm test`?',
    ...extra,
  };
}

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('LastTurn (Receipt tab)', () => {
  it('shows the sentence and the facts behind it', () => {
    act(() => root.render(<LastTurn summary={summary()} now={NOW} />));
    expect(host.querySelector('.ck-caption')?.textContent).toBe('Last turn');
    expect(host.querySelector('.ck-lastturn-line')?.textContent).toContain('edited 4 files (+86 −12), you approved 1 command');
    // the command is a code chip, never backticks on screen
    expect(host.querySelector('.ck-lastturn-line')?.textContent).toMatch(/now waiting: allow npm test\?$/);
    expect(host.querySelector('.ck-lastturn-line code')?.textContent).toBe('npm test');
    // this turn's file count is in the sentence only; the meta line must not repeat it
    expect(host.textContent).toContain('Turn 3 · 2m · 1m ago');
    expect(host.textContent).not.toContain('4 files ·');
    expect(host.querySelector('.ck-lastturn-line')?.getAttribute('title')).toBe('src/a.ts\nsrc/b.ts');
  });

  it('renders nothing before the first turn (the Receipt shows one empty state)', () => {
    act(() => root.render(<LastTurn summary={undefined} now={NOW} />));
    expect(host.textContent).toBe('');
  });

  it('leaves the file count out when it is unknown', () => {
    act(() => root.render(<LastTurn summary={summary({ files: null })} now={NOW} />));
    expect(host.textContent).toContain('Turn 3 · 2m · 1m ago');
  });
});

function Probe({ paneId }: { paneId: string }) {
  return <p id="probe">{useLastTurn(paneId)?.sentence ?? 'none'}</p>;
}

describe('latest summary per pane (sidebar tooltip source)', () => {
  it('reads the summary of its own pane from the context', () => {
    act(() =>
      root.render(
        <LastTurnContext.Provider value={new Map([['p1', summary()]])}>
          <Probe paneId="p1" />
          <Probe paneId="p2" />
        </LastTurnContext.Provider>,
      ),
    );
    const probes = host.querySelectorAll('#probe');
    expect(probes[0].textContent).toContain('edited 4 files');
    expect(probes[1].textContent).toBe('none');
  });
});

describe('useAway', () => {
  function harness() {
    let onSummary: (s: TurnSummary) => void = () => {};
    let onDigest: (d: Digest) => void = () => {};
    const api = {
      turnSummaries: vi.fn(async (id: string) => (id === 'p1' ? [summary({ turn: 2 }), summary({ turn: 3 })] : [])),
      onTurnSummary: vi.fn(async (cb: (s: TurnSummary) => void) => {
        onSummary = cb;
        return () => {};
      }),
      onDigestReady: vi.fn(async (cb: (d: Digest) => void) => {
        onDigest = cb;
        return () => {};
      }),
      awayDigestPending: vi.fn(async () => null),
      awayDigestDismiss: vi.fn(async () => {}),
      presenceCockpitFocus: vi.fn(async () => {}),
    };
    let state: ReturnType<typeof useAway> | undefined;
    function Hook({ ids }: { ids: string[] }) {
      state = useAway(ids, api);
      return null;
    }
    return { api, Hook, get: () => state!, emitSummary: (s: TurnSummary) => onSummary(s), emitDigest: (d: Digest) => onDigest(d) };
  }

  it('loads the newest summary per pane, follows the event and tells the PC the cockpit is open', async () => {
    const h = harness();
    await act(async () => root.render(<h.Hook ids={['p1', 'p2']} />));
    expect(h.get().lastTurn.get('p1')?.turn).toBe(3);
    expect(h.get().lastTurn.has('p2')).toBe(false);
    expect(h.api.presenceCockpitFocus).toHaveBeenCalledWith(true);

    await act(async () => h.emitSummary(summary({ turn: 4, sentence: 'newer' })));
    expect(h.get().lastTurn.get('p1')?.sentence).toBe('newer');
    // A late, older event never replaces a newer summary.
    await act(async () => h.emitSummary(summary({ turn: 2, sentence: 'stale' })));
    expect(h.get().lastTurn.get('p1')?.sentence).toBe('newer');
  });

  it('shows the digest when the PC says the user is back, and dismissing tells the PC', async () => {
    const h = harness();
    await act(async () => root.render(<h.Hook ids={[]} />));
    expect(h.get().digest).toBeNull();
    const digest: Digest = { sinceMs: NOW - 1000, nowMs: NOW, agentsWorked: 1, turns: 1, lines: [], needsYou: [] };
    await act(async () => h.emitDigest(digest));
    expect(h.get().digest).toEqual(digest);
    await act(async () => h.get().dismissDigest());
    expect(h.get().digest).toBeNull();
    expect(h.api.awayDigestDismiss).toHaveBeenCalled();
  });

  it('picks up a digest that was waiting before the window opened', async () => {
    const h = harness();
    const waiting: Digest = { sinceMs: 1, nowMs: 2, agentsWorked: 2, turns: 2, lines: [], needsYou: [] };
    h.api.awayDigestPending.mockResolvedValueOnce(waiting as never);
    await act(async () => root.render(<h.Hook ids={[]} />));
    expect(h.get().digest).toEqual(waiting);
  });
});

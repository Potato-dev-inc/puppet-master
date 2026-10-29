// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ciFixOffer, type WatchView } from '../../lib/ci-fix';
import type { ChecksReport, PrRef, ShipApi } from '../../lib/ship-api';
import { LABS_OFF_TEXT, MAX_POLLS, PrBlock } from './PrBlock';

vi.mock('../../lib/ci-fix', () => ({ ciFixOffer: vi.fn() }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const report = (state: ChecksReport['state'], names: string[] = []): ChecksReport => ({
  state,
  checked_at_ms: 1,
  items: names.map((name) => ({ name, state: state === 'failing' ? 'fail' : state === 'pending' ? 'pending' : 'pass' })),
});
const PR = (checks?: ChecksReport): PrRef => ({ number: 42, url: 'https://github.com/o/r/pull/42', draft: true, base: 'main', created_at_ms: 1, checks });
const WATCH: WatchView = {
  id: 'cw-1a2b3c4d',
  repo: 'o/r',
  prNumber: 42,
  prUrl: 'https://github.com/o/r/pull/42',
  branch: 'pm/cart-1',
  agentLabel: 'Codex',
  state: 'watching',
  stateText: 'Watching CI · 0 of 2 tries used',
  triesUsed: 0,
  triesMax: 2,
  lastFix: null,
  project: 'D:/code/shop',
  handle: 'cart-1',
};

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.mocked(ciFixOffer).mockReset();
  vi.mocked(ciFixOffer).mockResolvedValue(WATCH);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

const button = (text: string) => Array.from(document.querySelectorAll('button')).find((b) => b.textContent?.includes(text)) as HTMLButtonElement | undefined;
const settle = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });

function api(over: Partial<ShipApi> = {}): ShipApi {
  return {
    prChecks: vi.fn().mockResolvedValue(PR(report('passing', ['a']))),
    openUrl: vi.fn().mockResolvedValue(undefined),
    ...over,
  } as unknown as ShipApi;
}

async function mount(pr: PrRef, ship: ShipApi, extra: { onPr?: (p: PrRef) => void } = {}) {
  const onPr = extra.onPr ?? vi.fn();
  await act(async () => {
    root.render(<PrBlock repo="D:/code/shop" handle="cart-1" pr={pr} agentName="Codex" ship={ship} pollEveryMs={0} waitSecs={0} onPr={onPr} />);
  });
  await settle();
  return { onPr };
}

describe('the pull request block', () => {
  it('shows the link and the checks that are running', async () => {
    const ship = api({ prChecks: vi.fn().mockResolvedValue(PR(report('pending', ['a', 'b']))) });
    await mount(PR(report('pending', ['a', 'b'])), ship);
    expect(document.body.textContent).toContain('Draft PR #42');
    expect(document.body.textContent).toContain('GitHub checks are running (2 left)');
    await act(async () => button('Open on GitHub')?.click());
    expect(ship.openUrl).toHaveBeenCalledWith('https://github.com/o/r/pull/42');
  });

  it('polls a bounded number of times while checks stay pending, then stops', async () => {
    const ship = api({ prChecks: vi.fn().mockResolvedValue(PR(report('pending', ['a']))) });
    await mount(PR(), ship);
    // wait for the loop to run out
    await vi.waitFor(() => expect(document.body.textContent).toContain('Still running on GitHub'), { timeout: 3000 });
    expect((ship.prChecks as ReturnType<typeof vi.fn>).mock.calls.length).toBe(MAX_POLLS);
    expect(document.body.textContent).toContain('Still running on GitHub. Check again later.');
  });

  it('stops polling as soon as the checks settle', async () => {
    const ship = api();
    const { onPr } = await mount(PR(), ship);
    expect((ship.prChecks as ReturnType<typeof vi.fn>).mock.calls.length).toBe(1);
    expect(onPr).toHaveBeenCalledWith(expect.objectContaining({ checks: expect.objectContaining({ state: 'passing' }) }));
  });

  it('does not look again when the result is already known; passing checks offer Watch CI, not Fix', async () => {
    const ship = api();
    await mount(PR(report('passing', ['a', 'b'])), ship);
    expect(ship.prChecks).not.toHaveBeenCalled();
    expect(document.body.textContent).toContain('All 2 GitHub checks passed');
    expect(button('Fix with')).toBeUndefined();
    expect(button('Watch CI')).toBeTruthy();
  });

  it('names the failing checks and offers Fix with the agent, never asking Red to Green by itself', async () => {
    const ship = api();
    await mount(PR(report('failing', ['test (ubuntu)', 'lint'])), ship);
    expect(document.body.textContent).toContain('2 GitHub checks failed: test (ubuntu), lint');
    expect(button('Fix with Codex')).toBeTruthy();
    expect(button('Watch CI')).toBeUndefined();
    expect(ciFixOffer).not.toHaveBeenCalled();
  });

  it('Fix with asks Red to Green to watch this copy and shows the state it answers with', async () => {
    const ship = api();
    await mount(PR(report('failing', ['lint'])), ship);
    await act(async () => button('Fix with Codex')?.click());
    await settle();
    // The folder and the handle go to the backend; the card that follows is where the person says yes.
    expect(ciFixOffer).toHaveBeenCalledWith('D:/code/shop', 'cart-1');
    expect(document.body.textContent).toContain('Watching CI · 0 of 2 tries used');
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it('Watch CI on a PR that is not failing calls the same backend command', async () => {
    const ship = api();
    await mount(PR(report('passing', ['a'])), ship);
    await act(async () => button('Watch CI')?.click());
    await settle();
    expect(ciFixOffer).toHaveBeenCalledTimes(1);
    expect(ciFixOffer).toHaveBeenCalledWith('D:/code/shop', 'cart-1');
    expect(document.body.textContent).toContain('Watching CI · 0 of 2 tries used');
  });

  it('disables the button while the backend is answering', async () => {
    let answer: (w: WatchView) => void = () => {};
    vi.mocked(ciFixOffer).mockReturnValue(new Promise<WatchView>((resolve) => { answer = resolve; }));
    await mount(PR(report('failing', ['lint'])), api());
    await act(async () => button('Fix with Codex')?.click());
    expect(button('Fix with Codex')?.disabled).toBe(true);
    await act(async () => answer(WATCH));
    await settle();
    expect(button('Fix with Codex')?.disabled).toBe(false);
  });

  it('says where to turn Red to Green on when it is switched off', async () => {
    vi.mocked(ciFixOffer).mockRejectedValue('CI_FIX_OFF: Red to Green is switched off.');
    await mount(PR(report('failing', ['lint'])), api());
    await act(async () => button('Fix with Codex')?.click());
    await settle();
    expect(document.body.textContent).toContain(LABS_OFF_TEXT);
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it('shows other refusals in plain words', async () => {
    vi.mocked(ciFixOffer).mockRejectedValue(new Error('NO_PR: This copy has no pull request yet.'));
    await mount(PR(report('failing', ['lint'])), api());
    await act(async () => button('Fix with Codex')?.click());
    await settle();
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('no pull request yet');
  });

  it('shows a plain error when the checks cannot be read', async () => {
    const ship = api({ prChecks: vi.fn().mockRejectedValue(new Error('CHECKS_UNAVAILABLE: Could not read the GitHub checks: offline')) });
    await mount(PR(), ship);
    expect(document.body.textContent).toContain('Could not read the GitHub checks.');
    expect(document.querySelector('[role="alert"]')?.textContent).toContain('offline');
  });
});

// The words are built from pieces so this file does not hold the words it forbids.
describe('the old send-back path is gone from the frontend', () => {
  const SRC = path.resolve(__dirname, '../..');
  const FORBIDDEN = [['Send', 'back to'].join(' '), ['prSend', 'Back'].join(''), ['Send', 'BackPreview'].join('')];

  function sourceFiles(dir: string): string[] {
    return fs.readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const file = path.join(dir, entry.name);
      if (entry.isDirectory()) return sourceFiles(file);
      return /\.(ts|tsx|css)$/.test(entry.name) ? [file] : [];
    });
  }

  it('no file under src/ names the send-back sheet, its API or its preview type', () => {
    const hits = sourceFiles(SRC).flatMap((file) => {
      const text = fs.readFileSync(file, 'utf8');
      return FORBIDDEN.filter((word) => text.includes(word)).map((word) => `${path.relative(SRC, file)}: ${word}`);
    });
    expect(hits).toEqual([]);
  }, 60_000); // reads every source file, so it is slower than a unit test
});

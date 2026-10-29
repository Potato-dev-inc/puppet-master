// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ALWAYS_RED,
  BETWEEN_TURNS,
  CART_PATCH,
  COPY_DIFFERS,
  CULPRIT,
  CULPRIT_DIFF,
  FAILED,
  GOOD_WAS_RED,
  NOT_SURE_FLAKY,
  NOT_SURE_RUNS,
  SEVERAL,
  TESTS_PASS,
  UNDO_PLAN_CONFLICT,
  UNDO_PLAN_OK,
} from '../../dev/what-broke-it-result-fixtures';
import type { FindingView } from '../../lib/what-broke-it';
import { WhatBrokeItResult, type WhatBrokeItResultApi } from './WhatBrokeItResult';
import { lastGoodLine } from './what-broke-it-result-text';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const UNDONE_MS = 1_800_000_060_000;
const RECHECK_FINDING_ID = 'wb-55ee55ee55ee';

function mockApi(over: Partial<Record<keyof WhatBrokeItResultApi, unknown>> = {}) {
  const base = {
    wbDiff: vi.fn(async (_id: string, path?: string | null) => (path ? CART_PATCH : CULPRIT_DIFF)),
    wbUndoPlan: vi.fn(async () => UNDO_PLAN_OK),
    wbUndo: vi.fn(async () => ({ undoneAtMs: UNDONE_MS, written: 2, removed: 0 })),
    wbRecheck: vi.fn(async () => ({
      id: 'wbj-1',
      state: 'done',
      pointNo: 1,
      points: 1,
      run: 1,
      maxRuns: 1,
      elapsedMs: 900,
      maxMs: 60_000,
      line: '',
      findingId: RECHECK_FINDING_ID,
    })),
    wbStatus: vi.fn(async () => ({ id: 'wbj-1', state: 'done' })),
    wbFinding: vi.fn(async () => TESTS_PASS_RECHECK),
    ...over,
  };
  return base as unknown as Record<keyof WhatBrokeItResultApi, ReturnType<typeof vi.fn>> & WhatBrokeItResultApi;
}

/** What the recheck finishes with when the tests pass. */
const TESTS_PASS_RECHECK: FindingView = { ...TESTS_PASS, id: RECHECK_FINDING_ID };

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
  vi.restoreAllMocks();
});

async function mount(
  finding: FindingView,
  opts: { api?: ReturnType<typeof mockApi>; sharedWith?: string[] } = {},
) {
  const api = opts.api ?? mockApi();
  const notify = vi.fn();
  const onKeepLooking = vi.fn();
  await act(async () => {
    root.render(
      <WhatBrokeItResult
        finding={finding}
        api={api}
        sharedWith={opts.sharedWith ?? []}
        notify={notify}
        onKeepLooking={onKeepLooking}
      />,
    );
  });
  return { api, notify, onKeepLooking };
}

const text = () => host.textContent ?? '';
const buttons = () => Array.from(host.querySelectorAll('button'));
const button = (label: string) => buttons().find((b) => b.textContent?.trim() === label);
const buttonLabels = () => buttons().map((b) => b.textContent?.trim());

async function press(label: string) {
  const target = button(label);
  if (!target) throw new Error(`no button "${label}"`);
  await act(async () => {
    target.click();
  });
}

/** Let pending promise callbacks run. */
async function settle() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe('WhatBrokeItResult: the copy per kind', () => {
  it('culprit: title, time line, the broken test line, and the three buttons', async () => {
    await mount(CULPRIT);
    expect(host.querySelector('h2')?.textContent).toBe('Turn 7 by Codex broke the tests');
    expect(text()).toContain('14:10 · "refactor cart" · 4 files, +52 −20');
    expect(text()).toContain('It broke cart.test.ts and 1 more');
    expect(buttonLabels()).toEqual(['Show what it changed', 'Undo only this turn', 'Copy details']);
  });

  it('culprit: the last good time line shows only when the backend sends it', async () => {
    await mount(CULPRIT);
    expect(text()).not.toContain('Last time they passed');
    await mount({ ...CULPRIT, lastGoodMs: 1_800_000_000_000 });
    expect(text()).toContain(lastGoodLine(1_800_000_000_000));
  });

  it('several: the intro, one row per turn, and no undo', async () => {
    await mount(SEVERAL);
    expect(host.querySelector('h2')?.textContent).toBe('Two agents were working at the same time. It was one of these:');
    expect(text()).toContain('Codex · turn 6');
    expect(text()).toContain('Claude Code · turn 5');
    expect(text()).toContain("I can't tell which one.");
    expect(button('Undo only this turn')).toBeUndefined();
  });

  it('between turns: the sentence as given', async () => {
    await mount(BETWEEN_TURNS);
    expect(host.querySelector('h2')?.textContent).toBe(BETWEEN_TURNS.title);
    expect(text()).toContain('It broke between turns, not during one.');
    expect(button('Undo only this turn')).toBeUndefined();
  });

  it('not sure with runs used up: the reason, the range, and Keep looking', async () => {
    const { onKeepLooking } = await mount(NOT_SURE_RUNS);
    expect(host.querySelector('h2')?.textContent).toBe('Not sure. Ran out of runs.');
    expect(text()).toContain('It was one of turns 5 to 8 (Codex, Claude Code).');
    await press('Keep looking');
    expect(onKeepLooking).toHaveBeenCalledTimes(1);
  });

  it('not sure because of flaky tests: no Keep looking', async () => {
    await mount(NOT_SURE_FLAKY);
    expect(text()).toContain('Not sure. The tests pass and fail on the same files (they are flaky).');
    expect(button('Keep looking')).toBeUndefined();
  });

  it('always red, good was red, tests pass, copy differs and failed: the exact sentence', async () => {
    const cases: [FindingView, string][] = [
      [ALWAYS_RED, "The tests were already failing at the oldest save point (11:02). I can't look further back."],
      [GOOD_WAS_RED, 'The tests also fail at the save point you picked (11:02). Pick an earlier one, or choose Find it for me.'],
      [TESTS_PASS, "The tests pass in a clean copy. If they fail in your folder they may need files that save points don't keep (installed packages, .env). Add setup steps for the copy in Settings > Copies."],
      [COPY_DIFFERS, 'The test copy did not match the save point, so this answer is not trusted.'],
      [FAILED, 'npm was not found on this PC.'],
    ];
    for (const [finding, sentence] of cases) {
      act(() => root.unmount());
      root = createRoot(host);
      await mount(finding);
      expect(host.querySelector('h2')?.textContent).toBe(sentence);
      expect(buttonLabels()).toEqual(['Copy details']);
    }
  });
});

describe('WhatBrokeItResult: undo only this turn', () => {
  it('is offered only for a culprit that can be undone', async () => {
    await mount({ ...CULPRIT, canUndo: false });
    expect(button('Undo only this turn')).toBeUndefined();
    act(() => root.unmount());
    root = createRoot(host);
    await mount({ ...SEVERAL, canUndo: true });
    expect(button('Undo only this turn')).toBeUndefined();
  });

  it('asks with the turn, the file count and the save point line', async () => {
    const { api } = await mount(CULPRIT);
    await press('Undo only this turn');
    await settle();
    expect(api.wbUndoPlan).toHaveBeenCalledWith(CULPRIT.id);
    expect(host.querySelector('[role="alertdialog"] h2')?.textContent).toBe('Undo only turn 7?');
    expect(text()).toContain(
      "This puts back what turn 7 changed in 4 files. Work from later turns stays. It doesn't change what Codex remembers. A save point is made first, so you can undo this.",
    );
    expect(button('Undo turn 7')?.disabled).toBe(false);
    expect(button('Cancel')).toBeDefined();
  });

  it('adds the shared-folder warning when other agents work in the folder', async () => {
    await mount(CULPRIT, { sharedWith: ['Claude Code'] });
    await press('Undo only this turn');
    await settle();
    expect(text()).toContain('Claude Code also works in this folder. Its changes since then will be undone too.');
  });

  it('a conflict replaces the confirm text, names the file and blocks the button', async () => {
    await mount(CULPRIT, { api: mockApi({ wbUndoPlan: vi.fn(async () => UNDO_PLAN_CONFLICT) }) });
    await press('Undo only this turn');
    await settle();
    expect(text()).toContain(
      "Can't undo only this turn. A later change touched the same lines in cart.ts. Nothing was changed.",
    );
    expect(text()).not.toContain('This puts back what turn 7 changed');
    expect(button('Undo turn 7')?.disabled).toBe(true);
  });

  it('confirm writes the undo and then shows the undone line and Check it again', async () => {
    const { api } = await mount(CULPRIT);
    await press('Undo only this turn');
    await settle();
    await press('Undo turn 7');
    await settle();
    expect(api.wbUndo).toHaveBeenCalledWith(CULPRIT.id);
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(text()).toMatch(/Undone at \d\d:\d\d\./);
    expect(button('Undo only this turn')).toBeUndefined();
    expect(button('Check it again')).toBeDefined();
  });

  it('a refused undo shows the message in place of the confirm text', async () => {
    const api = mockApi({ wbUndo: vi.fn(async () => Promise.reject(new Error('Later work changed it.'))) });
    await mount(CULPRIT, { api });
    await press('Undo only this turn');
    await settle();
    await press('Undo turn 7');
    await settle();
    expect(text()).toContain('Later work changed it.');
    expect(host.querySelector('[role="alertdialog"]')).not.toBeNull();
  });

  it('an already undone culprit shows the line and Check it again, with no Undo button', async () => {
    await mount({ ...CULPRIT, undoneAtMs: UNDONE_MS });
    expect(text()).toMatch(/Undone at \d\d:\d\d\./);
    expect(button('Undo only this turn')).toBeUndefined();
    expect(button('Check it again')).toBeDefined();
  });
});

describe('WhatBrokeItResult: check it again', () => {
  it('says the tests pass when the new finding passes', async () => {
    const { api } = await mount({ ...CULPRIT, undoneAtMs: UNDONE_MS });
    await press('Check it again');
    await settle();
    expect(api.wbRecheck).toHaveBeenCalledWith(CULPRIT.id);
    expect(text()).toContain('The tests pass now.');
  });

  it('names the tests that still fail', async () => {
    const api = mockApi({
      wbFinding: vi.fn(async () => ({ ...CULPRIT, id: RECHECK_FINDING_ID, title: 'Still red', failing: ['cart.test.ts'] })),
    });
    await mount({ ...CULPRIT, undoneAtMs: UNDONE_MS }, { api });
    await press('Check it again');
    await settle();
    expect(text()).toContain('They still fail: cart.test.ts');
  });
});

describe('WhatBrokeItResult: details and files', () => {
  it('Copy details copies plain lines with no folder path', async () => {
    const writeText = vi.fn(async (_text: string) => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const leaky: FindingView = {
      ...CULPRIT,
      tail: 'expected 12.35\nat C:\\Users\\you\\dev\\cart\\cart.test.ts:14',
    };
    await mount(leaky);
    await press('Copy details');
    await settle();
    const copied = String(writeText.mock.calls[0]?.[0]);
    expect(copied).toContain('Turn 7 by Codex broke the tests');
    expect(copied).toContain('It broke cart.test.ts and 1 more');
    expect(copied).not.toContain('C:\\Users');
    expect(copied).not.toContain('you\\dev');
    expect(copied).toContain('~');
    expect(text()).toContain('Copied.');
  });

  it('Show what it changed lists the files, and a file shows its diff as plain text', async () => {
    const { api } = await mount(CULPRIT);
    await press('Show what it changed');
    await settle();
    expect(api.wbDiff).toHaveBeenCalledWith(CULPRIT.id);
    expect(text()).toContain('src/cart.ts');
    expect(text()).toContain('+40 −12');
    expect(text()).toContain('binary');
    const file = buttons().find((b) => b.textContent === 'src/cart.ts');
    await act(async () => {
      file?.click();
    });
    await settle();
    expect(api.wbDiff).toHaveBeenCalledWith(CULPRIT.id, 'src/cart.ts');
    expect(host.querySelector('pre')?.textContent).toContain('+export const rate = 0.125;');
  });

  it('shows test names as text, never as markup', async () => {
    const hostile: FindingView = { ...CULPRIT, failing: ['<img src=x onerror="alert(1)">'] };
    await mount(hostile);
    expect(host.querySelector('img')).toBeNull();
    expect(text()).toContain('It broke <img src=x onerror="alert(1)">');
  });
});

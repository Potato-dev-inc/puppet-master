// @vitest-environment jsdom
// Settings > General > Labs > Red to Green: the switch, the GitHub line, the four caps and the watched pull
// requests. `invoke` is mocked; every write must be one `ci_fix_save_settings` or one `ci_fix_act`.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CiFixSettings as CiFixConfig, CiFixView, FixView, GhView, WatchView } from '../../../lib/ci-fix';
import { blur, buttonByText, byLabel, click, settle, type as typeInto } from '../test-utils';
import { CAPS, CiFixSettings, FOOTER, ciGhLine, watchButtons } from './CiFixSettings';

const invoke = vi.hoisted(() => vi.fn());
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(async () => () => undefined) }));

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  invoke.mockReset();
  (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

const DEFAULTS: CiFixConfig = { enabled: false, tries_per_pr: 2, turns_per_try: 15, minutes_per_try: 20, fixes_per_day: 6 };
const READY: GhView = { state: 'ready', login: 'you' };
const NOTES = { lines: 0, cut_lines: 0, hidden_secrets: 0, invisible_chars: 0, jobs_read: 0, jobs_without_log: 0, truncated: false };

const fix = (over: Partial<FixView> = {}): FixView => ({
  id: 'fx-0123456789ab',
  watchId: 'cw-0a1b2c3d',
  tryNo: 1,
  state: 'declined',
  stateText: 'You said not now',
  checks: ['lint'],
  notes: NOTES,
  paneId: null,
  atMs: 1_000,
  ...over,
});

const watch = (over: Partial<WatchView> = {}): WatchView => ({
  id: 'cw-0a1b2c3d',
  repo: 'acme/billing',
  prNumber: 61,
  prUrl: 'https://github.com/acme/billing/pull/61',
  branch: 'pm/fix-login',
  agentLabel: 'Codex',
  state: 'watching',
  stateText: 'Watching CI',
  triesUsed: 0,
  triesMax: 2,
  lastFix: null,
  project: 'C:\\work\\billing',
  handle: 'cifix-61-a',
  ...over,
});

function view(over: Partial<CiFixView> = {}): CiFixView {
  return { settings: DEFAULTS, gh: READY, watches: [], fixes: [], ...over };
}

/** A fake backend: holds the view, applies saves and acts, and answers the reads. */
function backend(start: CiFixView) {
  const state = { view: start };
  invoke.mockImplementation(async (cmd: string, args?: { settings?: CiFixConfig }) => {
    switch (cmd) {
      case 'ci_fix_view':
        return state.view;
      case 'ci_fix_save_settings':
        state.view = { ...state.view, settings: args!.settings! };
        return state.view;
      case 'ci_fix_act':
        return state.view;
      default:
        throw new Error(`unexpected command ${cmd}`);
    }
  });
  return state;
}

async function mount(start: CiFixView) {
  backend(start);
  await act(async () => root.render(<CiFixSettings />));
  await settle(6);
}

const text = () => host.textContent ?? '';
const calls = (name: string) => invoke.mock.calls.filter((c) => c[0] === name);
const saves = (): CiFixConfig[] => calls('ci_fix_save_settings').map((c) => (c[1] as { settings: CiFixConfig }).settings);
const switchFor = (label: string) => byLabel<HTMLButtonElement>(host, label);
const rowOf = (name: string) => host.querySelector<HTMLElement>(`[aria-label="${name}"]`);
const buttonsIn = (el: HTMLElement | null): string[] => [...(el?.querySelectorAll('button') ?? [])].map((b) => b.textContent?.trim() ?? '');

describe('the switch and the GitHub line', () => {
  it('is off by default', async () => {
    await mount(view());
    expect(switchFor('Offer to fix failed CI')?.getAttribute('aria-checked')).toBe('false');
  });

  it('turning it on saves the whole settings object with enabled set and nothing else changed', async () => {
    const start = view({ settings: { ...DEFAULTS, tries_per_pr: 3, fixes_per_day: 4 } });
    await mount(start);
    await click(switchFor('Offer to fix failed CI'));
    await settle(3);
    expect(saves()).toEqual([{ ...start.settings, enabled: true }]);
  });

  it('names the GitHub login when gh is ready', async () => {
    await mount(view());
    expect(text()).toContain('Signed in to GitHub as @you (gh)');
  });

  it.each([
    ['missing', { state: 'missing', login: '' } as GhView],
    ['signed out', { state: 'signed_out', login: '' } as GhView],
  ])('says what to install when gh is %s', async (_name, gh) => {
    await mount(view({ gh }));
    expect(text()).toContain('Install the GitHub CLI and run gh auth login.');
    expect(text()).not.toContain('Signed in to GitHub');
  });

  it('ciGhLine names no login when there is none', () => {
    expect(ciGhLine({ state: 'ready', login: '' })).toBe('Signed in to GitHub (gh)');
    expect(ciGhLine({ state: 'ready', login: 'you' })).toBe('Signed in to GitHub as @you (gh)');
  });

  it('says why nothing shows when the desktop shell is not there', async () => {
    delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
    await act(async () => root.render(<CiFixSettings />));
    await settle(6);
    expect(text()).toContain('Could not read this setting: Desktop shell required: ci_fix_view');
  });
});

describe('the four caps', () => {
  it('show the saved values', async () => {
    await mount(view());
    expect(byLabel<HTMLInputElement>(host, 'Tries per pull request')?.value).toBe('2');
    expect(byLabel<HTMLInputElement>(host, 'Turns per try')?.value).toBe('15');
    expect(byLabel<HTMLInputElement>(host, 'Minutes per try')?.value).toBe('20');
    expect(byLabel<HTMLInputElement>(host, 'Fixes a day')?.value).toBe('6');
  });

  it('keep the ranges from the spec: tries 1-3, turns 1-50, minutes 5-60, a day 1-20', () => {
    expect(CAPS).toEqual({ tries: { min: 1, max: 3 }, turns: { min: 1, max: 50 }, minutes: { min: 5, max: 60 }, fixes: { min: 1, max: 20 } });
  });

  it.each([
    ['Tries per pull request', 'tries_per_pr', '9', 3],
    ['Tries per pull request', 'tries_per_pr', '0', 1],
    ['Turns per try', 'turns_per_try', '99', 50],
    ['Turns per try', 'turns_per_try', '0', 1],
    ['Minutes per try', 'minutes_per_try', '1', 5],
    ['Minutes per try', 'minutes_per_try', '90', 60],
    ['Fixes a day', 'fixes_per_day', '25', 20],
    ['Fixes a day', 'fixes_per_day', '0', 1],
  ] as const)('%s: typing %s saves %s', async (label, key, typed, expected) => {
    await mount(view());
    const input = byLabel<HTMLInputElement>(host, label)!;
    await typeInto(input, typed);
    await blur(input);
    await settle(3);
    expect(saves()).toHaveLength(1);
    expect(saves()[0][key]).toBe(expected);
  });

  it('a number that is already in range saves nothing', async () => {
    await mount(view());
    const input = byLabel<HTMLInputElement>(host, 'Turns per try')!;
    await typeInto(input, '15');
    await blur(input);
    await settle(3);
    expect(saves()).toEqual([]);
  });
});

describe('watched pull requests', () => {
  it('says so when none is watched yet', async () => {
    await mount(view());
    expect(text()).toContain('No pull requests yet.');
  });

  it.each([
    ['Watching CI', 'watching', 'Watching CI', ['Stop watching']],
    ['a pause', 'paused', 'Paused: someone else pushed to this branch', ['Stop watching', 'Watch again']],
    ['a stop', 'stopped', 'Stopped after 14 days', ['Watch again']],
    ['closed', 'closed', 'Closed', []],
    ['merged', 'merged', 'Merged', []],
  ] as const)('a %s row shows its backend text and its buttons', async (_name, state, stateText, buttons) => {
    await mount(view({ watches: [watch({ state, stateText })] }));
    const row = rowOf('#61 acme/billing');
    expect(row?.textContent).toContain(`#61 acme/billing · Codex · ${stateText}`);
    expect(buttonsIn(row)).toEqual([...buttons]);
  });

  it('a declined fix adds Ask again to its row', async () => {
    await mount(view({ watches: [watch({ lastFix: fix({ state: 'declined' }) })] }));
    expect(buttonsIn(rowOf('#61 acme/billing'))).toEqual(['Stop watching', 'Ask again']);
  });

  it('a fix still asking adds no button', async () => {
    await mount(view({ watches: [watch({ lastFix: fix({ state: 'asking' }) })] }));
    expect(buttonsIn(rowOf('#61 acme/billing'))).toEqual(['Stop watching']);
  });

  it('Stop watching, Watch again and Ask again call ci_fix_act with the watch id or the fix id', async () => {
    await mount(view({ watches: [watch({ state: 'paused', stateText: 'Paused', lastFix: fix({ state: 'expired' }) })] }));
    const row = rowOf('#61 acme/billing');
    await click(buttonByText(row!, 'Stop watching'));
    await click(buttonByText(row!, 'Watch again'));
    await click(buttonByText(row!, 'Ask again'));
    await settle(3);
    expect(calls('ci_fix_act').map((c) => c[1])).toEqual([
      { id: 'cw-0a1b2c3d', action: 'stop_watching' },
      { id: 'cw-0a1b2c3d', action: 'watch_again' },
      { id: 'fx-0123456789ab', action: 'ask_again' },
    ]);
  });

  it('watchButtons: a stopped watch offers Watch again only, a closed one offers nothing', () => {
    expect(watchButtons(watch({ state: 'stopped' }))).toEqual([{ label: 'Watch again', id: 'cw-0a1b2c3d', action: 'watch_again' }]);
    expect(watchButtons(watch({ state: 'closed' }))).toEqual([]);
  });
});

describe('the copy', () => {
  it('the footer says what happens and what does not', async () => {
    await mount(view());
    expect(text()).toContain(FOOTER);
    expect(FOOTER).toContain('It asks before the agent reads the log, and again before anything is pushed.');
    expect(FOOTER).toContain("The agent can't push during a fix.");
    expect(FOOTER).toContain('Puppet Master never re-runs CI, merges or force-pushes.');
  });

  it('the copy never says "auto", and "merge", "re-run" or "force" only in the footer that says PM never does them', async () => {
    await mount(view({ watches: [watch({ lastFix: fix({ state: 'declined' }) })] }));
    const rest = text().replace(FOOTER, '');
    expect(rest).not.toMatch(/auto/i);
    expect(rest).not.toMatch(/merge/i);
    expect(rest).not.toMatch(/re-run/i);
    expect(rest).not.toMatch(/force/i);
    expect(text()).not.toMatch(/auto-fix|autopilot|self-healing/i);
  });

  it('shows no snake_case words on the page', async () => {
    await mount(view({ watches: [watch()] }));
    expect(text()).not.toMatch(/\b[a-z]+_[a-z]+\b/);
  });
});

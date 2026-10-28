// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { PaneUsage } from '../../../lib/pane-usage';
import { HelpersCard, type HelperRow } from './HelpersCard';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const helpers: HelperRow[] = [{ id: 'h1', title: 'Check tests', agent: 'Codex', status: 'running' }];

const usage = (withTokens: boolean): PaneUsage => ({
  own: { turns: 2, busyMs: 0 },
  helperCount: 1,
  helperTurns: 6,
  helperBusyMs: 4 * 60_000,
  helpers: [
    {
      paneId: 'h1',
      accountLabel: 'work',
      turns: 6,
      busyMs: 4 * 60_000,
      ...(withTokens ? { tokens: { input: 12_000, output: 3_000 } } : {}),
    },
  ],
});

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

describe('HelpersCard usage lines', () => {
  it('shows the roll-up and the helper line with tokens when the chat reported them', () => {
    act(() => root.render(<HelpersCard helpers={helpers} usage={usage(true)} onOpen={() => {}} />));
    const text = host.textContent ?? '';
    expect(host.querySelector('[data-testid="rail-helpers-rollup"]')?.textContent).toBe('Helpers: 1 · 6 turns · 4 min');
    expect(text).toContain('Codex · 6 turns · 4 min · work account · 12k in, 3k out tokens');
  });

  it('shows the helper line without any token words when the chat reported none', () => {
    act(() => root.render(<HelpersCard helpers={helpers} usage={usage(false)} onOpen={() => {}} />));
    const text = host.textContent ?? '';
    expect(text).toContain('Codex · 6 turns · 4 min · work account');
    expect(text).not.toContain('tokens');
    expect(text).not.toContain(' in,');
  });

  it('shows no usage words until the usage is known', () => {
    act(() => root.render(<HelpersCard helpers={helpers} usage={null} onOpen={() => {}} />));
    const text = host.textContent ?? '';
    expect(text).toContain('Check tests');
    expect(text).not.toContain('turns');
    expect(host.querySelector('[data-testid="rail-helpers-rollup"]')).toBeNull();
  });

  it('keeps the list of helpers and the open click when usage is shown', () => {
    const open = vi.fn();
    act(() => root.render(<HelpersCard helpers={helpers} usage={usage(false)} onOpen={open} />));
    act(() => host.querySelector<HTMLButtonElement>('.ck-rail-item')!.click());
    expect(open).toHaveBeenCalledWith('h1');
  });

  it('offers "Save this team" only when the app can save one', () => {
    act(() => root.render(<HelpersCard helpers={helpers} onOpen={() => {}} />));
    expect(host.textContent).not.toContain('Save this team');
    const save = vi.fn();
    act(() => root.render(<HelpersCard helpers={helpers} onOpen={() => {}} onSaveTeam={save} />));
    const button = [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent === 'Save this team')!;
    act(() => button.click());
    expect(save).toHaveBeenCalledTimes(1);
  });

  it('shows nothing, not even the button, when there are no helpers', () => {
    act(() => root.render(<HelpersCard helpers={[]} onOpen={() => {}} onSaveTeam={() => {}} />));
    expect(host.textContent).toBe('');
  });
});

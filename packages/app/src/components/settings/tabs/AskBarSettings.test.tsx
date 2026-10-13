// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AskbarAgent, AskbarApi, AskbarSettings } from '../../../lib/askbar';
import { AskBarSettingsBlock } from './AskBarSettings';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

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

const AGENTS: AskbarAgent[] = [
  { id: 'claude', label: 'Claude Code', ready: true },
  { id: 'codex', label: 'Codex', ready: false },
];

function fakeApi(initial: AskbarSettings) {
  return {
    settingsGet: vi.fn(async () => initial),
    settingsSet: vi.fn(async (next: AskbarSettings) => next),
    agents: vi.fn(async () => AGENTS),
  };
}

const settings = (over: Partial<AskbarSettings> = {}): AskbarSettings => ({
  enabled: false,
  shortcut: 'Ctrl+Alt+Space',
  default_agent: null,
  last_agent: null,
  ...over,
});

const mount = async (api: Partial<AskbarApi>) => {
  await act(async () => {
    root.render(<AskBarSettingsBlock api={api as AskbarApi} />);
  });
};
const toggle = () => host.querySelector('button[role="switch"]') as HTMLButtonElement;
const selects = () => Array.from(host.querySelectorAll('select'));
const pick = async (select: HTMLSelectElement, value: string) => {
  await act(async () => {
    select.value = value;
    select.dispatchEvent(new Event('change', { bubbles: true }));
  });
};

describe('AskBarSettingsBlock', () => {
  it('is off by default and shows both selects disabled', async () => {
    await mount(fakeApi(settings()));
    expect(toggle().getAttribute('aria-checked')).toBe('false');
    expect(host.textContent).toContain('Off by default.');
    expect(selects()).toHaveLength(2);
    expect(selects().every((s) => s.disabled)).toBe(true);
  });

  it('enabling calls settingsSet with enabled: true and enables the selects', async () => {
    const api = fakeApi(settings());
    await mount(api);
    await act(async () => toggle().click());
    expect(api.settingsSet).toHaveBeenCalledWith(expect.objectContaining({ enabled: true }));
    expect(toggle().getAttribute('aria-checked')).toBe('true');
    expect(selects().every((s) => !s.disabled)).toBe(true);
  });

  it('choosing a shortcut and an agent saves those values', async () => {
    const api = fakeApi(settings({ enabled: true }));
    await mount(api);
    expect(Array.from(selects()[1].options).map((o) => o.textContent)).toEqual(['Last used', 'Claude Code', 'Codex']);
    await pick(selects()[0], 'Ctrl+Shift+Space');
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ shortcut: 'Ctrl+Shift+Space', enabled: true }));
    await pick(selects()[1], 'codex');
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ default_agent: 'codex' }));
  });

  it('choosing Last used clears the default agent', async () => {
    const api = fakeApi(settings({ enabled: true, default_agent: 'claude' }));
    await mount(api);
    expect(selects()[1].value).toBe('claude');
    await pick(selects()[1], '');
    expect(api.settingsSet).toHaveBeenLastCalledWith(expect.objectContaining({ default_agent: null }));
  });
});

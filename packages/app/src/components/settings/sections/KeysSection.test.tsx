// @vitest-environment jsdom
// Settings > Keys: every control renders, secrets save on blur (never on a timer), OpenCode calls carry the right
// arguments, and removing keys is confirmed and comes last.
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { blur, buttonByText, byLabel, click, hasText, mountRows, settle, type as typeInto, type Mounted } from '../test-utils';
import { keysRows } from './KeysSection';

const AUTOMATION = "Switch to another of your keys when one hits its rate limit (check your provider's terms)";

const mocks = vi.hoisted(() => ({
  secrets: {
    storageInfo: vi.fn<() => Promise<{ secure: boolean; label: string }>>(),
    getMasked: vi.fn<(name: string) => Promise<{ set: boolean; last4: string | null }>>(),
    set: vi.fn(async (_name: string, _value: string) => undefined),
    remove: vi.fn(async (_name: string) => undefined),
  },
  stored: { anthropic: false },
  opencode: {
    status: {
      active_profile: 'a',
      profiles: [
        { id: 'a', label: 'Primary', configured: true },
        { id: 'b', label: 'Secondary', configured: false },
      ],
      // Off: switching to another key at a rate limit is opt-in.
      key_automation_mode: 'notify_only',
      restart_pane_on_rotate: true,
    },
  },
}));

vi.mock('../../../lib/secrets', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../../../lib/secrets')>()),
  secretsApi: mocks.secrets,
}));
vi.mock('../../../lib/tauri', () => ({
  tauri: {
    getOpenCodeKeyStatus: vi.fn(async () => structuredClone(mocks.opencode.status)),
    setOpenCodeKeyProfile: vi.fn(async () => undefined),
    setOpenCodeAutomationSettings: vi.fn(async (mode: string, restart: boolean) => ({
      ...mocks.opencode.status,
      key_automation_mode: mode,
      restart_pane_on_rotate: restart,
    })),
  },
}));

import { tauri } from '../../../lib/tauri';

let m: Mounted;

async function mount(overrides: Parameters<typeof mountRows>[1] = {}) {
  m = await mountRows(keysRows, overrides);
  await settle(5);
  return m;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.stored.anthropic = false;
  mocks.secrets.storageInfo.mockImplementation(async () => ({ secure: true, label: 'Windows Credential Manager' }));
  mocks.secrets.getMasked.mockImplementation(async (name: string) =>
    name === 'anthropic_api_key' && mocks.stored.anthropic ? { set: true, last4: 'WXYZ' } : { set: false, last4: null },
  );
});
afterEach(async () => {
  await m?.unmount();
  vi.useRealTimers();
});

const rowFor = (id: string) => m.host.querySelector<HTMLElement>(`[data-row="${id}"]`);

describe('Keys: every control renders', () => {
  it('shows every live control of the old API tab that belongs here', async () => {
    await mount();
    for (const label of [
      'Anthropic API key',
      'OpenAI API key',
      'OpenRouter API key',
      'OpenCode API keys (A / B)',
      AUTOMATION,
      'Restart pane after key rotate',
      'Remove all keys',
    ]) {
      expect(hasText(m.host, label), `missing: ${label}`).toBe(true);
    }
    for (const aria of ['Anthropic API key', 'OpenAI API key', 'OpenRouter API key', 'OpenCode key A', 'OpenCode key B']) {
      expect(byLabel<HTMLInputElement>(m.host, aria), `missing field: ${aria}`).toBeTruthy();
    }
    expect(byLabel(m.host, AUTOMATION)).toBeTruthy();
    expect(byLabel(m.host, 'Restart pane after key rotate')).toBeTruthy();
    expect(buttonByText(m.host, 'Refresh status')).toBeTruthy();
    expect(buttonByText(m.host, 'Remove…')).toBeTruthy();
  });

  it('has no Save button and no planned controls (GitHub, Linear, validate on save)', async () => {
    await mount();
    expect(buttonByText(m.host, 'Save')).toBeUndefined();
    expect(buttonByText(m.host, 'Save OpenCode keys')).toBeUndefined();
    expect(buttonByText(m.host, 'Save changes')).toBeUndefined();
    expect(hasText(m.host, 'Planned')).toBe(false);
    expect(hasText(m.host, 'GitHub token')).toBe(false);
    expect(hasText(m.host, 'Linear API key')).toBe(false);
    expect(hasText(m.host, 'Validate keys on save')).toBe(false);
  });

  it('puts the destructive row in its own last list', async () => {
    await mount();
    const lists = [...m.host.querySelectorAll('section.st-group')];
    const last = lists[lists.length - 1];
    expect(last.getAttribute('data-danger')).toBe('true');
    expect(last.textContent).toContain('Remove all keys');
    expect(lists.slice(0, -1).some((s) => s.textContent?.includes('Remove all keys'))).toBe(false);
  });
});

describe('Keys: provider keys save on blur, not on a timer', () => {
  it('typing a key then leaving the field saves it through the secure store and clears the field', async () => {
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'Anthropic API key')!;
    await typeInto(field, 'sk-ant-test-1234');
    expect(mocks.secrets.set).not.toHaveBeenCalled();
    await blur(field);
    await settle(5);
    expect(mocks.secrets.set).toHaveBeenCalledExactlyOnceWith('anthropic_api_key', 'sk-ant-test-1234');
    expect(m.env.markSaved).toHaveBeenCalled();
    expect(m.env.patches).toEqual([]); // secure keys never touch settings.json
  });

  it('a key typed and left alone is never saved by a timer', async () => {
    vi.useFakeTimers();
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'OpenAI API key')!;
    await typeInto(field, 'sk-test-9999');
    await act(async () => void (await vi.advanceTimersByTimeAsync(10_000)));
    expect(mocks.secrets.set).not.toHaveBeenCalled();
  });

  it('Enter saves the key too', async () => {
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'OpenRouter API key')!;
    await typeInto(field, 'sk-or-test-5678');
    await act(async () => {
      field.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
    await settle(5);
    expect(mocks.secrets.set).toHaveBeenCalledExactlyOnceWith('openrouter_api_key', 'sk-or-test-5678');
  });

  it('removing one stored key asks first; declining keeps it', async () => {
    mocks.stored.anthropic = true;
    await mount();
    m.env.confirm.mockResolvedValueOnce(false);
    await click(buttonByText(m.host, 'Remove'));
    await settle();
    expect(m.env.confirm).toHaveBeenCalledWith(expect.objectContaining({ title: 'Remove the Anthropic key?', confirmLabel: 'Remove' }));
    expect(mocks.secrets.remove).not.toHaveBeenCalled();
  });

  it('removing one stored key after confirming deletes it from the store', async () => {
    mocks.stored.anthropic = true;
    await mount();
    await click(buttonByText(m.host, 'Remove'));
    await settle();
    expect(mocks.secrets.remove).toHaveBeenCalledExactlyOnceWith('anthropic_api_key');
    expect(m.env.markSaved).toHaveBeenCalled();
  });
});

describe('Keys: OpenCode A / B keys', () => {
  it('loads the status on mount and shows each key line', async () => {
    await mount();
    expect(tauri.getOpenCodeKeyStatus).toHaveBeenCalled();
    expect(hasText(m.host, 'Key A - Configured - active')).toBe(true);
    expect(hasText(m.host, 'Key B - Not set')).toBe(true);
  });

  it('a key typed into A is saved on blur as the Primary profile, then the status refreshes', async () => {
    await mount();
    const before = vi.mocked(tauri.getOpenCodeKeyStatus).mock.calls.length;
    const field = byLabel<HTMLInputElement>(m.host, 'OpenCode key A')!;
    await typeInto(field, 'sk-oc-a-123');
    expect(tauri.setOpenCodeKeyProfile).not.toHaveBeenCalled();
    await blur(field);
    await settle(5);
    expect(tauri.setOpenCodeKeyProfile).toHaveBeenCalledExactlyOnceWith('a', 'sk-oc-a-123', 'Primary');
    expect(vi.mocked(tauri.getOpenCodeKeyStatus).mock.calls.length).toBeGreaterThan(before);
    expect(m.env.markSaved).toHaveBeenCalled();
    expect(field.value).toBe('');
  });

  it('key B saves as the Secondary profile', async () => {
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'OpenCode key B')!;
    await typeInto(field, 'sk-oc-b-456');
    await blur(field);
    await settle(5);
    expect(tauri.setOpenCodeKeyProfile).toHaveBeenCalledExactlyOnceWith('b', 'sk-oc-b-456', 'Secondary');
  });

  it('a blur with an empty key field saves nothing', async () => {
    await mount();
    await blur(byLabel<HTMLInputElement>(m.host, 'OpenCode key A')!);
    await settle();
    expect(tauri.setOpenCodeKeyProfile).not.toHaveBeenCalled();
  });

  it('a failed key save shows the error on the row', async () => {
    await mount();
    vi.mocked(tauri.setOpenCodeKeyProfile).mockRejectedValueOnce(new Error('keychain locked'));
    const field = byLabel<HTMLInputElement>(m.host, 'OpenCode key A')!;
    await typeInto(field, 'sk-oc-a-789');
    await blur(field);
    await settle(5);
    expect(m.env.setError).toHaveBeenCalledWith('Not saved: keychain locked');
    expect(m.env.markSaved).not.toHaveBeenCalled();
  });

  it('Refresh status asks for the status again', async () => {
    await mount();
    const before = vi.mocked(tauri.getOpenCodeKeyStatus).mock.calls.length;
    await click(buttonByText(m.host, 'Refresh status'));
    await settle();
    expect(vi.mocked(tauri.getOpenCodeKeyStatus).mock.calls.length).toBe(before + 1);
  });
});

describe('Keys: key automation', () => {
  it('is off by default and the restart switch is enabled once the status has loaded', async () => {
    await mount();
    const automation = byLabel<HTMLButtonElement>(m.host, AUTOMATION)!;
    expect(automation.disabled).toBe(false);
    expect(automation.getAttribute('aria-checked')).toBe('false');
    expect(byLabel<HTMLButtonElement>(m.host, 'Restart pane after key rotate')!.disabled).toBe(false);
  });

  it('the label says the provider terms apply and there is no select any more', async () => {
    await mount();
    expect(hasText(m.host, "check your provider's terms")).toBe(true);
    expect(byLabel(m.host, 'Automation mode')).toBeFalsy();
  });

  it('turning it on saves auto_if_backup with the current restart setting; turning it off saves notify_only', async () => {
    await mount();
    await click(byLabel<HTMLButtonElement>(m.host, AUTOMATION));
    await settle();
    expect(tauri.setOpenCodeAutomationSettings).toHaveBeenLastCalledWith('auto_if_backup', true);
    expect(m.env.markSaved).toHaveBeenCalled();
    expect(byLabel<HTMLButtonElement>(m.host, AUTOMATION)!.getAttribute('aria-checked')).toBe('true');
    await click(byLabel<HTMLButtonElement>(m.host, AUTOMATION));
    await settle();
    expect(tauri.setOpenCodeAutomationSettings).toHaveBeenLastCalledWith('notify_only', true);
    expect(m.env.patches).toEqual([]);
  });

  it('a person who opted in earlier sees it on, and the restart switch keeps that choice', async () => {
    mocks.opencode.status.key_automation_mode = 'auto_if_backup';
    await mount();
    expect(byLabel<HTMLButtonElement>(m.host, AUTOMATION)!.getAttribute('aria-checked')).toBe('true');
    await click(byLabel<HTMLButtonElement>(m.host, 'Restart pane after key rotate'));
    await settle();
    expect(tauri.setOpenCodeAutomationSettings).toHaveBeenCalledExactlyOnceWith('auto_if_backup', false);
    mocks.opencode.status.key_automation_mode = 'notify_only';
  });

  it('the restart switch never turns key switching on', async () => {
    await mount();
    await click(byLabel<HTMLButtonElement>(m.host, 'Restart pane after key rotate'));
    await settle();
    expect(tauri.setOpenCodeAutomationSettings).toHaveBeenCalledExactlyOnceWith('notify_only', false);
  });

  it('a failed automation save shows the error on the row', async () => {
    await mount();
    vi.mocked(tauri.setOpenCodeAutomationSettings).mockRejectedValueOnce(new Error('pane busy'));
    await click(byLabel<HTMLButtonElement>(m.host, 'Restart pane after key rotate'));
    await settle();
    expect(m.env.setError).toHaveBeenCalledWith('Not saved: pane busy');
  });
});

describe('Keys: remove all keys (destructive, last list)', () => {
  it('declining the confirm removes nothing', async () => {
    await mount();
    m.env.confirm.mockResolvedValueOnce(false);
    await click(buttonByText(m.host, 'Remove…'));
    await settle();
    expect(m.env.confirm).toHaveBeenCalledWith(expect.objectContaining({ title: 'Remove all keys?', confirmLabel: 'Remove all' }));
    expect(mocks.secrets.remove).not.toHaveBeenCalled();
    expect(m.env.patches).toEqual([]);
  });

  it('confirming removes all three keys from the store and clears them from settings', async () => {
    await mount();
    await click(buttonByText(m.host, 'Remove…'));
    await settle(5);
    expect(mocks.secrets.remove.mock.calls.map((c) => c[0])).toEqual(['anthropic_api_key', 'openai_api_key', 'openrouter_api_key']);
    expect(m.env.patches).toEqual([{ anthropic_api_key: null, openai_api_key: null, openrouter_api_key: null }]);
    expect(m.env.markSaved).toHaveBeenCalled();
    expect(tauri.setOpenCodeKeyProfile).not.toHaveBeenCalled(); // OpenCode keys are left alone
  });

  it('without a secure store it only clears the settings file', async () => {
    mocks.secrets.storageInfo.mockImplementation(async () => ({ secure: false, label: 'memory' }));
    await mount();
    m.env.confirm.mockResolvedValueOnce(true);
    await click(buttonByText(m.host, 'Remove…'));
    await settle(5);
    expect(mocks.secrets.remove).not.toHaveBeenCalled();
    expect(m.env.patches).toEqual([{ anthropic_api_key: null, openai_api_key: null, openrouter_api_key: null }]);
  });

  it('is not shown as a planned or greyed-out action', async () => {
    await mount();
    expect(buttonByText(m.host, 'Remove…')!.disabled).toBe(false);
    expect(rowFor('remove-all-keys')).toBeTruthy();
  });
});

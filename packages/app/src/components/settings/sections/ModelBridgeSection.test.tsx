// @vitest-environment jsdom
// Settings > Orchestrator & Models > Model Bridge: connect a service, preview, add to Codex, undo, remove, plain errors,
// and the row is reachable in the real Settings (only while the Labs switch is on).
import { createRoot } from 'react-dom/client';
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { ModelBridgeApi, ModelBridgeList, ModelBridgeService } from '../../../lib/model-bridge';
import { searchSections, visibleSections } from '../model';
import { SECTIONS } from '../catalog';
import { blur, buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type as typeInto } from '../test-utils';
import { ModelBridge, modelBridgeRows } from './ModelBridgeSection';

const KEY = 'sk-test-SECRET-1234567890abcdef';

const GO: ModelBridgeService = {
  id: 'opencode-go',
  displayName: 'OpenCode Go',
  note: "Check OpenCode's terms for your plan.",
  limitsNote: 'A monthly dollar limit.',
  needsReview: true,
  blank: false,
  baseUrl: 'https://opencode.ai/inference/go/openai/v1',
  shapes: ['responses'],
  keySaved: false,
  codexDirect: true,
  applied: null,
  envLine: '$env:OPENCODE_API_KEY = "<paste your key here>"',
};
const BLANK: ModelBridgeService = { ...GO, id: 'any-responses', displayName: 'Any service that speaks the Responses API', note: null, needsReview: false, blank: true, baseUrl: '', limitsNote: null };

interface Fake {
  api: ModelBridgeApi;
  state: ModelBridgeList;
  calls: string[];
}

function fake(overrides: Partial<ModelBridgeApi> = {}): Fake {
  const state: ModelBridgeList = { enabled: true, secure: true, storageLabel: 'Windows Credential Manager', adapters: [{ ...GO }, { ...BLANK }], problems: [] };
  const calls: string[] = [];
  const set = (id: string, patch: Partial<ModelBridgeService>) => {
    state.adapters = state.adapters.map((a) => (a.id === id ? { ...a, ...patch } : a));
  };
  const api: ModelBridgeApi = {
    list: async () => structuredClone(state),
    saveKey: async (id, key) => {
      calls.push(`saveKey:${id}:${key}`);
      set(id, { keySaved: true });
      return state.storageLabel;
    },
    remove: async (id) => {
      calls.push(`remove:${id}`);
      set(id, { keySaved: false, applied: null });
    },
    addCustom: async (name, url) => {
      calls.push(`addCustom:${name}:${url}`);
      state.adapters.push({ ...GO, id: 'custom-box', displayName: name, note: null, needsReview: false, baseUrl: url });
      return 'custom-box';
    },
    preview: async (id) => {
      calls.push(`preview:${id}`);
      return {
        title: 'PM will add a provider to your Codex settings. A backup is kept.',
        configPath: 'C:\\Users\\a\\.codex\\config.toml',
        lines: ['[model_providers.pm-opencode-go]', 'wire_api = "responses"'],
        envLine: GO.envLine,
        service: 'OpenCode Go',
      };
    },
    apply: async (id) => {
      calls.push(`apply:${id}`);
      set(id, { applied: { recipe: 'codex', envVar: 'OPENCODE_API_KEY' } });
      return { message: 'Codex can now use OpenCode Go.', backupPath: 'C:\\x.bak' };
    },
    undo: async (id) => {
      calls.push(`undo:${id}`);
      set(id, { applied: null });
      return { message: 'Your Codex settings are back exactly as they were.', backupPath: null };
    },
    ...overrides,
  };
  return { api, state, calls };
}

let host: HTMLDivElement;
let root: ReturnType<typeof createRoot>;

async function mount(api: ModelBridgeApi) {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  await act(async () => root.render(<ModelBridge api={api} />));
  await settle(5);
}

beforeEach(() => vi.clearAllMocks());
afterEach(async () => {
  await act(async () => root?.unmount());
  host?.remove();
});

const input = (label: string) => byLabel<HTMLInputElement>(host, label)!;

async function connectGo(f: Fake) {
  await click(buttonByText(host, 'Connect a model service'));
  await typeInto(input('Service key'), KEY);
  await click(buttonByText(host, 'Save key'));
  await settle(5);
  expect(f.calls).toContain(`saveKey:opencode-go:${KEY}`);
}

describe('Model Bridge section', () => {
  it('starts with a Connect button and no services', async () => {
    await mount(fake().api);
    expect(buttonByText(host, 'Connect a model service')).toBeTruthy();
    expect(host.querySelector('[data-service]')).toBeNull();
  });

  it('connects a service: pick, paste the key once, see where it is kept, then a Ready row', async () => {
    const f = fake();
    await mount(f.api);
    await click(buttonByText(host, 'Connect a model service'));
    expect(hasText(host, "Check OpenCode's terms for your plan.")).toBe(true);
    expect(input('Service key').type).toBe('password');
    expect(buttonByText(host, 'Save key')!.disabled).toBe(true);
    await typeInto(input('Service key'), KEY);
    await click(buttonByText(host, 'Save key'));
    await settle(5);
    expect(hasText(host, 'Key saved in Windows Credential Manager.')).toBe(true);
    const row = host.querySelector('[data-service="opencode-go"]')!;
    expect(row.textContent).toContain('Ready');
    expect(row.textContent).toContain('Use in Codex');
    expect(row.textContent).toContain('Remove');
    // The key is gone from the window: not in the form, not anywhere in the page.
    expect(host.textContent).not.toContain('SECRET');
    expect(host.querySelector('input[type="password"]')).toBeNull();
  });

  it('adds a service of your own with a name and an address', async () => {
    const f = fake();
    await mount(f.api);
    await click(buttonByText(host, 'Connect a model service'));
    await pickOption(host.querySelector('select[aria-label="Service"]')!, 'any-responses');
    expect(buttonByText(host, 'Save key')!.disabled).toBe(true);
    await typeInto(input('Service name'), 'My Box');
    await typeInto(input('Service address'), 'https://box.example.test/v1');
    await typeInto(input('Service key'), KEY);
    await click(buttonByText(host, 'Save key'));
    await settle(5);
    expect(f.calls[0]).toBe('addCustom:My Box:https://box.example.test/v1');
    expect(f.calls[1]).toBe(`saveKey:custom-box:${KEY}`);
    expect(host.querySelector('[data-service="custom-box"]')).not.toBeNull();
  });

  it('Use in Codex shows the preview card; Cancel changes nothing', async () => {
    const f = fake();
    await mount(f.api);
    await connectGo(f);
    await click(byLabel(host, 'Use OpenCode Go in Codex'));
    await settle(3);
    expect(hasText(host, 'PM will add a provider to your Codex settings. A backup is kept.')).toBe(true);
    expect(host.querySelector('pre')!.textContent).toContain('[model_providers.pm-opencode-go]');
    expect(hasText(host, '$env:OPENCODE_API_KEY = "<paste your key here>"')).toBe(true);
    expect(host.textContent).not.toContain('SECRET');
    await click(buttonByText(host, 'Cancel'));
    expect(f.calls.some((c) => c.startsWith('apply'))).toBe(false);
    expect(host.querySelector('[aria-label="Preview"]')).toBeNull();
  });

  it('Add writes it, says Codex can use the service, and Undo puts it back', async () => {
    const f = fake();
    await mount(f.api);
    await connectGo(f);
    await click(byLabel(host, 'Use OpenCode Go in Codex'));
    await settle(3);
    await click(buttonByText(host, 'Add'));
    await settle(5);
    expect(f.calls).toContain('apply:opencode-go');
    expect(hasText(host, 'Codex can now use OpenCode Go.')).toBe(true);
    expect(host.querySelector('[data-service="opencode-go"]')!.textContent).toContain('In your Codex settings');
    await click(buttonByText(host, 'Undo'));
    await settle(5);
    expect(f.calls).toContain('undo:opencode-go');
    expect(hasText(host, 'Your Codex settings are back exactly as they were.')).toBe(true);
    expect(host.querySelector('[data-service="opencode-go"]')!.textContent).toContain('Use in Codex');
  });

  it('Remove forgets the service', async () => {
    const f = fake();
    await mount(f.api);
    await connectGo(f);
    await click(byLabel(host, 'Remove OpenCode Go'));
    await settle(5);
    expect(f.calls).toContain('remove:opencode-go');
    expect(host.querySelector('[data-service]')).toBeNull();
    expect(hasText(host, 'Its key is gone from Windows Credential Manager.')).toBe(true);
  });

  it('shows the app\'s plain error, including the translator line', async () => {
    const f = fake({
      preview: async () => {
        throw 'This service needs the translator (coming next).';
      },
    });
    await mount(f.api);
    await connectGo(f);
    await click(byLabel(host, 'Use OpenCode Go in Codex'));
    await settle(3);
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('This service needs the translator (coming next).');
    expect(host.querySelector('[aria-label="Preview"]')).toBeNull();
  });

  it('a key that cannot be saved shows a plain error and keeps no key on screen', async () => {
    const f = fake({
      saveKey: async () => {
        throw new Error('The key could not be saved in the secure key store.');
      },
    });
    await mount(f.api);
    await click(buttonByText(host, 'Connect a model service'));
    await typeInto(input('Service key'), KEY);
    await click(buttonByText(host, 'Save key'));
    await settle(5);
    expect(host.querySelector('[role="alert"]')!.textContent).toBe('The key could not be saved in the secure key store.');
    expect(host.textContent).not.toContain('SECRET');
  });

  it('says so when the secure key store is missing', async () => {
    const f = fake();
    f.state.secure = false;
    await mount(f.api);
    expect(hasText(host, 'The secure key store on this computer is not available')).toBe(true);
    expect(buttonByText(host, 'Connect a model service')!.disabled).toBe(true);
  });
});

describe('Model Bridge is reachable in the real Settings', () => {
  const models = (flags: { model_bridge_enabled?: boolean } | null) => visibleSections(SECTIONS, flags).find((s) => s.id === 'models')!;

  it('is a row of Orchestrator & Models while the Labs switch is on, and gone while it is off', () => {
    expect(models({ model_bridge_enabled: true }).rows.map((r) => r.id)).toContain('model-bridge');
    expect(models({ model_bridge_enabled: false }).rows.map((r) => r.id)).not.toContain('model-bridge');
    expect(models(null).rows.map((r) => r.id)).not.toContain('model-bridge');
  });

  it('is found by search, and the Labs switch is in General > Labs', () => {
    const hits = searchSections(visibleSections(SECTIONS, { model_bridge_enabled: true }), 'model bridge');
    expect(JSON.stringify(hits)).toContain('model-bridge');
    const labs = SECTIONS.find((s) => s.id === 'general')!.rows.filter((r) => r.labs?.name === 'Model Bridge');
    expect(labs.length).toBe(1);
  });

  it('the real row renders the section, in monochrome rows', async () => {
    vi.resetModules();
    const m = await mountRows(modelBridgeRows);
    await settle(6);
    // Outside the desktop app the list call fails; the row still renders and shows a plain message, not a crash.
    expect(m.host.querySelector('[data-row="model-bridge"]')).not.toBeNull();
    await m.unmount();
  });

  it('the Labs toggle patches model_bridge_enabled', async () => {
    const row = SECTIONS.find((s) => s.id === 'general')!.rows.find((r) => r.labs?.name === 'Model Bridge')!;
    const m = await mountRows([row]);
    const toggle = m.host.querySelector('[role="switch"], input[type="checkbox"]') as HTMLElement;
    await click(toggle);
    expect(m.env.patches).toContainEqual({ model_bridge_enabled: true });
    await m.unmount();
    void blur;
  });
});

// @vitest-environment jsdom
// Settings > Orchestrator & Models: every control renders, saves the right partial patch, and search finds the rows.
import { act } from 'react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LlmModel, Settings } from '@puppet-master/shared';
import { rowMatches, searchSections, type SectionDef } from '../model';
import { BASE_SETTINGS, blur, buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type as typeInto, type Mounted } from '../test-utils';
import { modelsRows } from './ModelsSection';

const CUSTOM_CLAUDE: LlmModel = { provider: 'anthropic', model_id: 'my-claude', label: 'My Claude' };
const CUSTOM_OR: LlmModel = { provider: 'openrouter', model_id: 'x/y', label: 'X model' };

let m: Mounted;

async function mount(settings: Partial<Settings> = {}) {
  m = await mountRows(modelsRows, { settings: { ...BASE_SETTINGS, ...settings } });
  await settle(5);
  return m;
}

beforeEach(() => vi.clearAllMocks());
afterEach(async () => {
  await m?.unmount();
  vi.useRealTimers();
});

const ids = modelsRows.map((r) => r.id);

describe('Orchestrator & Models: every control renders', () => {
  it('has one row per control, each in its own [data-row] wrapper', async () => {
    await mount({ custom_models: [CUSTOM_CLAUDE, CUSTOM_OR] });
    expect(ids).toEqual(['orchestrator-backend', 'librarian-indexer', 'opencode-provider', 'opencode-model', 'default-provider', 'default-model', 'custom-models']);
    expect(new Set(ids).size).toBe(ids.length);
    for (const id of ids) expect(m.host.querySelector(`[data-row="${id}"]`), `no row ${id}`).not.toBeNull();
  });

  it('shows every control by its label and aria-label', async () => {
    await mount({ custom_models: [CUSTOM_CLAUDE, CUSTOM_OR] });
    for (const label of ['Default orchestrator', 'Librarian indexer', 'OpenCode provider', 'OpenCode model', 'Provider', 'Model', 'Custom models']) {
      expect(hasText(m.host, label), `missing label: ${label}`).toBe(true);
    }
    for (const aria of [
      'Default orchestrator',
      'Librarian indexer',
      'OpenCode provider ID',
      'OpenCode model ID',
      'Default provider',
      'Default model',
      'Custom model provider',
      'Custom model display label',
      'Custom model ID',
      'Remove My Claude',
    ]) {
      expect(byLabel(m.host, aria), `missing control: ${aria}`).not.toBeNull();
    }
    expect(buttonByText(m.host, 'Add')).toBeTruthy();
  });

  it('groups the rows under their headings in order', async () => {
    await mount();
    const heads = [...m.host.querySelectorAll('section.st-group h2')].map((h) => h.textContent);
    expect(heads).toEqual(['Orchestrator', 'OpenCode', 'Model for the API orchestrator']);
  });

  it('has no Save button, no planned controls and no destructive list', async () => {
    await mount();
    expect(buttonByText(m.host, 'Save changes')).toBeUndefined();
    expect(hasText(m.host, 'Planned')).toBe(false);
    expect(hasText(m.host, 'Not implemented')).toBe(false);
    expect(modelsRows.some((r) => r.danger)).toBe(false);
    expect(m.host.querySelector('[data-danger="true"]')).toBeNull();
  });
});

describe('Orchestrator & Models: each control saves a partial patch', () => {
  it('Default orchestrator saves only {orchestrator_backend}', async () => {
    await mount();
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Default orchestrator')!, 'codex_cli');
    expect(m.env.patches).toEqual([{ orchestrator_backend: 'codex_cli' }]);
  });

  it('Provider saves the provider and its first model', async () => {
    await mount();
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Default provider')!, 'openai');
    expect(m.env.patches).toEqual([{ default_provider: 'openai', default_model: 'gpt-4.1' }]);
    await m.rerender();
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Default provider')!, 'openrouter');
    expect(m.env.patches.at(-1)).toEqual({ default_provider: 'openrouter', default_model: 'anthropic/claude-sonnet-4' });
  });

  it('Model lists only the current provider and saves only {default_model}', async () => {
    await mount({ default_provider: 'openai', default_model: 'gpt-4.1' });
    const model = byLabel<HTMLSelectElement>(m.host, 'Default model')!;
    expect([...model.options].map((o) => o.value)).toEqual(['gpt-4.1', 'gpt-4.1-mini']);
    await pickOption(model, 'gpt-4.1-mini');
    expect(m.env.patches).toEqual([{ default_model: 'gpt-4.1-mini' }]);
  });

  it('Custom models: Add is disabled until label and id are filled, then saves the three keys and clears the draft', async () => {
    await mount();
    expect(buttonByText(m.host, 'Add')!.disabled).toBe(true);
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Custom model provider')!, 'openrouter');
    await typeInto(byLabel<HTMLInputElement>(m.host, 'Custom model display label')!, 'My router');
    expect(buttonByText(m.host, 'Add')!.disabled).toBe(true);
    await typeInto(byLabel<HTMLInputElement>(m.host, 'Custom model ID')!, 'acme/router-1');
    expect(buttonByText(m.host, 'Add')!.disabled).toBe(false);
    await click(buttonByText(m.host, 'Add'));
    await settle();
    expect(m.env.patches).toEqual([
      {
        custom_models: [{ provider: 'openrouter', model_id: 'acme/router-1', label: 'My router' }],
        default_provider: 'openrouter',
        default_model: 'acme/router-1',
      },
    ]);
    expect(byLabel<HTMLInputElement>(m.host, 'Custom model ID')!.value).toBe('');
    expect(byLabel<HTMLInputElement>(m.host, 'Custom model display label')!.value).toBe('');
  });

  it('Remove saves only {custom_models} with the rest of the list and asks nothing', async () => {
    await mount({ custom_models: [CUSTOM_CLAUDE, CUSTOM_OR] });
    await click(byLabel(m.host, 'Remove My Claude'));
    await settle();
    expect(m.env.patches).toEqual([{ custom_models: [CUSTOM_OR] }]);
    expect(m.env.confirm).not.toHaveBeenCalled();
  });

  it('Librarian indexer clears to null after a pause in typing', async () => {
    vi.useFakeTimers();
    await mount({ librarian_indexer_path: 'opencode' });
    const field = byLabel<HTMLInputElement>(m.host, 'Librarian indexer')!;
    expect(field.value).toBe('opencode');
    await typeInto(field, '');
    expect(m.env.patches).toHaveLength(0);
    await act(async () => void (await vi.advanceTimersByTimeAsync(700)));
    expect(m.env.patches).toEqual([{ librarian_indexer_path: null }]);
  });

  it('Librarian indexer saves a typed path after a pause', async () => {
    vi.useFakeTimers();
    await mount({ librarian_indexer_path: 'opencode' });
    await typeInto(byLabel<HTMLInputElement>(m.host, 'Librarian indexer')!, 'scripts/project-indexer.py');
    await act(async () => void (await vi.advanceTimersByTimeAsync(700)));
    expect(m.env.patches).toEqual([{ librarian_indexer_path: 'scripts/project-indexer.py' }]);
  });

  it('OpenCode provider and model save on blur, and clear to null', async () => {
    await mount({ opencode_model_provider: 'anthropic', opencode_model_id: 'claude-sonnet-4' });
    const modelId = byLabel<HTMLInputElement>(m.host, 'OpenCode model ID')!;
    await typeInto(modelId, 'claude-opus-4');
    await blur(modelId);
    expect(m.env.patches).toEqual([{ opencode_model_id: 'claude-opus-4' }]);

    const provider = byLabel<HTMLInputElement>(m.host, 'OpenCode provider ID')!;
    await typeInto(provider, '');
    await blur(provider);
    expect(m.env.patches.at(-1)).toEqual({ opencode_model_provider: null });
  });
});

describe('Orchestrator & Models: search', () => {
  const section: SectionDef = { id: 'models', title: 'Orchestrator & Models', blurb: '', rows: modelsRows };

  it('finds custom models for "openrouter"', () => {
    const hits = searchSections([section], 'openrouter');
    expect(hits[0].rows.map((r) => r.id)).toContain('custom-models');
  });

  it('finds rows by their old words', () => {
    const row = (id: string) => modelsRows.find((r) => r.id === id)!;
    expect(rowMatches(row('librarian-indexer'), 'indexer')).toBe(true);
    expect(rowMatches(row('orchestrator-backend'), 'codex')).toBe(true);
    expect(rowMatches(row('orchestrator-backend'), 'opencode cli')).toBe(true);
    expect(rowMatches(row('custom-models'), 'custom model')).toBe(true);
    expect(rowMatches(row('librarian-indexer'), 'openrouter')).toBe(false);
  });
});

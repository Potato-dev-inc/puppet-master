// @vitest-environment jsdom
// Settings > Potato Bot > Models and fallbacks: the talk brain choice (the same control as before, now here), the ordered
// fallback list (add, move, remove; "Through my agents" turns it off), a plain sentence when a save fails, and no key field.
import { act, type ReactNode } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport, type PotatoSettings } from '../../../lib/potato';
import { settle, buttonByText, byLabel, click, hasText, pickOption } from '../test-utils';
import { ModelsPage, moveBrain } from './PotatoModelsPage';

let host: HTMLElement | null = null;
let unmount: (() => Promise<void>) | null = null;

async function mountPage(node: ReactNode): Promise<HTMLElement> {
  host = document.createElement('div');
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => root.render(node));
  await settle(6);
  unmount = async () => {
    await act(async () => root.unmount());
    host?.remove();
  };
  return host;
}

afterEach(async () => {
  await unmount?.();
  unmount = null;
  host = null;
});

function seeded(settings: Partial<PotatoSettings> = {}): FakePotatoTransport {
  return fakePotatoTransport({ state: { settings: { v: 1, brain: 'deepseek', defaultPersona: 'fish', dailyLimitCents: 50, fallbacks: [], ...settings } as PotatoSettings } });
}

const saved = (fake: FakePotatoTransport) => fake.calls.filter((c) => c.method === 'settingsSet').at(-1)?.args[0] as PotatoSettings | undefined;

describe('Settings > Potato Bot > Models and fallbacks', () => {
  it('has the talk brain choice with its three brains, explains the one picked, and saves it in the Potato settings', async () => {
    const fake = seeded();
    const root = await mountPage(<ModelsPage transport={fake} />);
    const select = byLabel<HTMLSelectElement>(root, 'Talk brain')!;
    expect([...select.options].map((o) => o.textContent)).toEqual(['DeepSeek', 'OpenCode Go', 'Through my agents']);
    expect(select.value).toBe('deepseek');
    expect(hasText(root, 'Pays DeepSeek per message. Usually cents a day.')).toBe(true);

    await pickOption(select, 'go');
    await settle(4);
    expect(saved(fake)).toMatchObject({ brain: 'go', dailyLimitCents: 50 });
    expect(hasText(root, 'Uses your OpenCode Go sign-in.')).toBe(true);
  });

  it('starts with no fallback and says what happens then', async () => {
    const root = await mountPage(<ModelsPage transport={seeded()} />);
    expect(hasText(root, 'None yet. If the talk brain fails, you will see the error.')).toBe(true);
    expect(root.querySelector('ol.pot-fb-list')).toBeNull();
  });

  it('adds a fallback from the brains that are not the talk brain, and saves the list', async () => {
    const fake = seeded({ brain: 'deepseek' });
    const root = await mountPage(<ModelsPage transport={fake} />);
    const add = byLabel<HTMLSelectElement>(root, 'Add a fallback')!;
    expect([...add.options].map((o) => o.textContent)).toEqual(['Choose a brain', 'OpenCode Go']);
    expect(buttonByText(root, 'Add')?.disabled).toBe(true);

    await pickOption(add, 'go');
    await click(buttonByText(root, 'Add'));
    await settle(4);
    expect(saved(fake)).toMatchObject({ brain: 'deepseek', fallbacks: ['go'] });
    expect(root.querySelector('[aria-label="Move OpenCode Go up"]')).not.toBeNull();
  });

  it('with one fallback there is nothing to move, and Remove saves the list without it', async () => {
    const fake = seeded({ brain: 'deepseek', fallbacks: ['go'] });
    const root = await mountPage(<ModelsPage transport={fake} />);
    expect((byLabel<HTMLButtonElement>(root, 'Move OpenCode Go up') as HTMLButtonElement).disabled).toBe(true);
    expect((byLabel<HTMLButtonElement>(root, 'Move OpenCode Go down') as HTMLButtonElement).disabled).toBe(true);
    expect(hasText(root, 'Every other brain is in the list.')).toBe(true);

    await click(byLabel(root, 'Remove OpenCode Go'));
    await settle(4);
    expect(saved(fake)?.fallbacks).toEqual([]);
    expect(hasText(root, 'None yet.')).toBe(true);
  });

  it('is off for "Through my agents": the list cannot change and it says why', async () => {
    const root = await mountPage(<ModelsPage transport={seeded({ brain: 'agents', fallbacks: ['go'] })} />);
    expect(hasText(root, 'Fallbacks do not apply to Through my agents.')).toBe(true);
    expect(byLabel(root, 'Add a fallback')).toBeNull();
    expect((byLabel<HTMLButtonElement>(root, 'Remove OpenCode Go') as HTMLButtonElement).disabled).toBe(true);
  });

  it('a save that fails says why, in plain words', async () => {
    const fake = seeded();
    fake.settingsSet = () => Promise.reject(new Error('POTATO_STORE_INVALID: settings.json is damaged.'));
    const root = await mountPage(<ModelsPage transport={fake} />);
    await pickOption(byLabel<HTMLSelectElement>(root, 'Talk brain')!, 'go');
    await settle(4);
    expect(root.querySelector('[role="alert"].st-note')?.textContent).toBe('settings.json is damaged.');
  });

  it('shows no key field: keys stay where they are', async () => {
    const root = await mountPage(<ModelsPage transport={seeded({ fallbacks: ['go'] })} />);
    expect(root.querySelector('input[type="password"]')).toBeNull();
    expect(root.textContent).not.toMatch(/sk-/);
  });

  it('moveBrain puts one brain in a new place and keeps the others in order', () => {
    expect(moveBrain(['go', 'deepseek', 'agents'], 2, 0)).toEqual(['agents', 'go', 'deepseek']);
    expect(moveBrain(['go', 'deepseek'], 0, 1)).toEqual(['deepseek', 'go']);
    expect(moveBrain(['go', 'deepseek'], 0, 0)).toEqual(['go', 'deepseek']);
    expect(moveBrain(['go'], 0, 1), 'out of range: unchanged').toEqual(['go']);
  });
});

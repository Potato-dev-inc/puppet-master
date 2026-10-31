// @vitest-environment jsdom
// Settings > Potato Bot: the key shows only as "set" and its last 4, never the value; the switch; the brain choice;
// the daily limit stays between $0.05 and $20; Check key says Works. or one plain sentence.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakePotatoTransport, type FakePotatoTransport } from '../../../lib/potato';
import type { SecretsApi } from '../../../lib/secrets';
import { visibleSections } from '../model';
import { blur, buttonByText, byLabel, click, hasText, mountRows, pickOption, settle, type as typeInto, unsearchableLabels, type Mounted } from '../test-utils';
import { SECTIONS } from '../catalog';
import { limitCentsOf, makePotatoRows } from './PotatoSection';

const SECRET = 'sk-live-deepseek-0123456789-1a2b';

let m: Mounted;
let fake: FakePotatoTransport;
let stored: { set: boolean; last4: string | null };
let api: SecretsApi & { setCalls: [string, string][]; removeCalls: string[] };

beforeEach(() => {
  fake = fakePotatoTransport();
  stored = { set: true, last4: '1a2b' };
  const setCalls: [string, string][] = [];
  const removeCalls: string[] = [];
  api = {
    setCalls,
    removeCalls,
    storageInfo: async () => ({ secure: true, label: 'Windows Credential Manager' }),
    getMasked: async () => ({ ...stored }),
    set: async (name, value) => {
      setCalls.push([name, value]);
      stored = { set: true, last4: value.slice(-4) };
    },
    remove: async (name) => {
      removeCalls.push(name);
      stored = { set: false, last4: null };
    },
  };
});
afterEach(async () => {
  await m?.unmount();
});

async function mount(over: Parameters<typeof mountRows>[1] = {}) {
  m = await mountRows(makePotatoRows(fake, api), { settings: { ...(over.settings ?? {}), potato_bot_enabled: true } as never, ...over });
  await settle(6);
  return m;
}

const calls = (method: string) => fake.calls.filter((c) => c.method === method);

describe('Settings > Potato Bot', () => {
  it('shows every row, in the order of the spec, with the exact copy', async () => {
    await mount();
    expect([...m.host.querySelectorAll('[data-row]')].map((el) => el.getAttribute('data-row'))).toEqual(['potato-bot', 'potato-brain', 'potato-key', 'potato-persona', 'potato-limit']);
    for (const text of ['Potato Bot', 'Talk brain', 'DeepSeek key', 'Check key', 'Default persona', 'Daily limit', 'Pays DeepSeek per message. Usually cents a day.']) {
      expect(hasText(m.host, text), text).toBe(true);
    }
    expect(m.host.querySelector('[data-row="potato-key"] .st-label')?.textContent ?? '').not.toContain(SECRET);
  });

  it('shows the key as set with its last 4 characters and never the value', async () => {
    await mount();
    expect(m.host.querySelector('[data-testid="key-state"]')?.textContent).toContain('•••• 1a2b');
    expect(m.host.textContent).not.toContain(SECRET);
    expect(m.host.innerHTML).not.toContain(SECRET);
    // no input shows while a key is set: Replace and Remove are the controls
    expect(byLabel(m.host, 'DeepSeek key')).toBeNull();
    expect(buttonByText(m.host, 'Replace')).toBeTruthy();
    expect(buttonByText(m.host, 'Remove')).toBeTruthy();
  });

  it('a typed key goes to the keychain under its own name, clears the field, and is not shown or sent to the page transport', async () => {
    stored = { set: false, last4: null };
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'DeepSeek key')!;
    expect(field.type).toBe('password');
    await typeInto(field, SECRET);
    await blur(field);
    await settle(4);
    expect(api.setCalls).toEqual([['deepseek_api_key', SECRET]]);
    expect(m.host.textContent).toContain('•••• 1a2b');
    expect(m.host.innerHTML).not.toContain(SECRET);
    // the Potato commands never carried it
    expect(JSON.stringify(fake.calls)).not.toContain(SECRET);
  });

  it('Remove deletes the key from the keychain', async () => {
    await mount();
    await click(buttonByText(m.host, 'Remove'));
    await settle(4);
    expect(api.removeCalls).toEqual(['deepseek_api_key']);
    expect(m.host.textContent).toContain('Not set.');
  });

  it('the switch is the kill switch: it saves potato_bot_enabled, and only that', async () => {
    await mount();
    const sw = byLabel<HTMLButtonElement>(m.host, 'Potato Bot')!;
    expect(sw.getAttribute('aria-checked')).toBe('true');
    await click(sw);
    expect(m.env.patches).toEqual([{ potato_bot_enabled: false }]);
  });

  it('the brain choice offers the three brains, explains each, saves it in the Potato settings and leaves the settings file alone', async () => {
    await mount();
    const select = byLabel<HTMLSelectElement>(m.host, 'Talk brain')!;
    expect([...select.options].map((o) => o.textContent)).toEqual(['DeepSeek', 'OpenCode Go', 'Through my agents']);
    expect(select.value).toBe('deepseek');
    await pickOption(select, 'go');
    await settle(4);
    expect(calls('settingsSet').at(-1)?.args[0]).toMatchObject({ brain: 'go', dailyLimitCents: 50 });
    expect(hasText(m.host, 'Uses your OpenCode Go sign-in.')).toBe(true);
    // a brain with no DeepSeek key: the key row says it is not needed and shows no field
    expect(byLabel(m.host, 'DeepSeek key')).toBeNull();
    expect(buttonByText(m.host, 'Check key')).toBeUndefined();
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Talk brain')!, 'agents');
    await settle(4);
    expect(hasText(m.host, 'No extra cost.')).toBe(true);
    expect(calls('settingsSet').at(-1)?.args[0]).toMatchObject({ brain: 'agents' });
    expect(m.env.patches).toEqual([]);
    expect(m.env.markSaved).toHaveBeenCalled();
  });

  it('the default persona is Fish or Potato', async () => {
    await mount();
    const select = byLabel<HTMLSelectElement>(m.host, 'Default persona')!;
    expect([...select.options].map((o) => o.value)).toEqual(['fish', 'potato']);
    await pickOption(select, 'potato');
    await settle(4);
    expect(calls('settingsSet').at(-1)?.args[0]).toMatchObject({ defaultPersona: 'potato' });
  });

  it('the daily limit starts at $0.50 and stays between $0.05 and $20', async () => {
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'Daily limit in dollars')!;
    expect(field.value).toBe('0.5');
    expect(field.min).toBe('0.05');
    expect(field.max).toBe('20');
    const saved = async (typed: string) => {
      await typeInto(field, typed);
      await blur(field);
      await settle(4);
      return calls('settingsSet').at(-1)?.args[0] as { dailyLimitCents: number };
    };
    expect((await saved('2')).dailyLimitCents).toBe(200);
    expect((await saved('100')).dailyLimitCents).toBe(2000);
    expect(field.value).toBe('20');
    expect((await saved('0.001')).dailyLimitCents).toBe(5);
    expect(field.value).toBe('0.05');
    const before = calls('settingsSet').length;
    await typeInto(field, 'lots');
    await blur(field);
    await settle(4);
    expect(calls('settingsSet').length, 'not a number: nothing saved').toBe(before);
    expect(field.value).toBe('0.05');
  });

  it('limitCentsOf rounds to whole cents inside the range', () => {
    expect([0, 0.04, 0.05, 0.5, 0.505, 19.99, 20, 300].map(limitCentsOf)).toEqual([5, 5, 5, 50, 51, 1999, 2000, 2000]);
  });

  it('Check key says Works. or one plain sentence without the code', async () => {
    await mount();
    await click(buttonByText(m.host, 'Check key'));
    await settle(4);
    expect(calls('checkKey').length).toBe(1);
    expect(m.host.querySelector('[role="status"].st-note')?.textContent).toBe('Works.');
    fake = Object.assign(fake, {});
    await m.unmount();
    const failing = fakePotatoTransport({
      keyCheck: () => {
        throw new Error('POTATO_KEY_CHECK: DeepSeek did not accept that key.');
      },
    });
    m = await mountRows(makePotatoRows(failing, api), { settings: { potato_bot_enabled: true } as never });
    await settle(6);
    await click(buttonByText(m.host, 'Check key'));
    await settle(4);
    expect(m.host.querySelector('[role="alert"].st-note')?.textContent).toBe('DeepSeek did not accept that key.');
  });

  it('says why a save did not work, in plain words', async () => {
    const broken = fakePotatoTransport();
    broken.settingsSet = () => Promise.reject(new Error('POTATO_STORE_INVALID: settings.json is damaged.'));
    m = await mountRows(makePotatoRows(broken, api), { settings: { potato_bot_enabled: true } as never });
    await settle(6);
    await pickOption(byLabel<HTMLSelectElement>(m.host, 'Talk brain')!, 'go');
    await settle(4);
    expect(m.env.setError).toHaveBeenLastCalledWith('settings.json is damaged.');
  });

  it('every word of every label finds its row in search', async () => {
    await mount();
    expect(unsearchableLabels(m.host, makePotatoRows(fake, api))).toEqual([]);
  });
});

describe('the Potato Bot section in the catalog', () => {
  it('is in the list but shown only while the Labs switch is on', () => {
    expect(SECTIONS.some((s) => s.id === 'potato')).toBe(true);
    expect(visibleSections(SECTIONS, null).some((s) => s.id === 'potato')).toBe(false);
    expect(visibleSections(SECTIONS, {}).some((s) => s.id === 'potato')).toBe(false);
    expect(visibleSections(SECTIONS, { potato_bot_enabled: false }).some((s) => s.id === 'potato')).toBe(false);
    expect(visibleSections(SECTIONS, { potato_bot_enabled: true }).map((s) => s.id)).toContain('potato');
    expect(visibleSections(SECTIONS, { potato_bot_enabled: true }).filter((s) => s.id !== 'potato').length).toBe(6);
  });

  it('the switch also lives in General > Labs, where a person turns it on', () => {
    const general = SECTIONS.find((s) => s.id === 'general')!;
    const labs = general.rows.find((r) => r.id === 'potato-labs');
    expect(labs?.labs?.name).toBe('Potato Bot');
  });
});

describe('a computer with no secure storage', () => {
  it('never puts the key in the settings file: it says the key cannot be kept', async () => {
    api.storageInfo = async () => ({ secure: false, label: 'none' });
    await mount();
    const field = byLabel<HTMLInputElement>(m.host, 'DeepSeek key')!;
    await typeInto(field, SECRET);
    await blur(field);
    await settle(4);
    expect(m.env.patches).toEqual([]);
    expect(api.setCalls).toEqual([]);
    expect(m.host.textContent).toContain('keeps its key only in the system keychain');
    expect(m.host.textContent).not.toContain(SECRET);
  });
});

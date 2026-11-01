// The starter templates: every data file must pass the schema and carry no secret-like or memory fields, and
// `makeStarterBot` must call only the existing create, add, save, tools and routine calls.
import { describe, expect, it, vi } from 'vitest';
import { fakeCompanionTransport } from './companion';
import { fakePotatoTransport } from './potato';
import { fakeBotSettingsTransport } from './potato-botsettings';
import { fakeRoutinesTransport } from './potato-routines';
import { STARTERS, loadStarterFiles, makeStarterBot, starterSchema, type StarterTemplate, type StarterTransports } from './potato-starters';

const SECRET_KEY = /key|token|secret|password|passwd|credential|memory|soul|apikey/i;
const SECRET_VALUE = /sk-[A-Za-z0-9]{8,}|Bearer\s+\S+|-----BEGIN/;

function keysDeep(value: unknown, path = ''): string[] {
  if (Array.isArray(value)) return value.flatMap((v, i) => keysDeep(v, `${path}[${i}]`));
  if (value && typeof value === 'object') {
    return Object.entries(value).flatMap(([k, v]) => [`${path}.${k}`, ...keysDeep(v, `${path}.${k}`)]);
  }
  return [];
}

function transports() {
  const companions = fakeCompanionTransport();
  const potato = fakePotatoTransport({ bots: [] });
  const settings = fakeBotSettingsTransport();
  const routines = fakeRoutinesTransport();
  const set = {
    companions: { agents: vi.spyOn(companions, 'agents'), create: vi.spyOn(companions, 'create') },
    potato: { addBot: vi.spyOn(potato, 'addBot') },
    settings: { saveInstructions: vi.spyOn(settings, 'saveInstructions'), setTools: vi.spyOn(settings, 'setTools') },
    routines: { create: vi.spyOn(routines, 'create') },
  };
  const t: StarterTransports = { companions, potato, settings, routines };
  return { t, set };
}

describe('starter template files', () => {
  it('has six files, one per card, in the order the picker shows', () => {
    expect(loadStarterFiles()).toHaveLength(6);
    expect(STARTERS.map((s) => s.name)).toEqual(['Inbox helper', 'Research buddy', 'Daily planner', 'Website tester', 'Code watcher', 'Blank']);
  });

  it.each(loadStarterFiles().map((f) => [f.path, f.data] as const))('%s validates against the schema', (_path, data) => {
    expect(starterSchema.safeParse(data).success).toBe(true);
  });

  it.each(loadStarterFiles().map((f) => [f.path, f.data] as const))('%s has no secret-like keys, values or memory', (_path, data) => {
    for (const key of keysDeep(data)) {
      expect(key, key).not.toMatch(SECRET_KEY);
    }
    expect(JSON.stringify(data)).not.toMatch(SECRET_VALUE);
  });

  it('only Blank has no routine, and no template has a routine it cannot run', () => {
    for (const s of STARTERS) {
      if (s.id === 'blank') {
        expect(s.routine).toBeUndefined();
        expect(s.instructions).toBeUndefined();
      } else {
        expect(s.routine, s.id).toBeDefined();
        expect(s.instructions, s.id).toBeDefined();
      }
    }
  });

  it('marks computer-needing templates exactly when they switch on the computer or web reading', () => {
    for (const s of STARTERS) {
      expect(s.needsComputer, s.id).toBe(!!(s.tools && (s.tools.computer || s.tools.web)));
    }
    expect(STARTERS.filter((s) => s.needsComputer).map((s) => s.id).sort()).toEqual(['research-buddy', 'website-tester']);
  });

  it('rejects an extra field such as a key, so a bad file cannot slip in', () => {
    const bad = { ...STARTERS[0], apiKey: 'x' };
    expect(starterSchema.safeParse(bad).success).toBe(false);
  });
});

describe('makeStarterBot', () => {
  it('creates the companion with the template name and face, adds the bot, saves instructions, sets tools and makes the routine', async () => {
    const { t, set } = transports();
    const inbox = STARTERS.find((s) => s.id === 'inbox-helper') as StarterTemplate;
    const botId = await makeStarterBot(inbox, t);
    expect(botId).toBeTruthy();
    expect(set.companions.create).toHaveBeenCalledWith(expect.objectContaining({ name: 'Inbox helper', avatar: 'tofu', agentType: 'claude', folder: '' }));
    expect(set.potato.addBot).toHaveBeenCalledWith(botId);
    expect(set.settings.saveInstructions).toHaveBeenCalledWith(botId, inbox.instructions);
    expect(set.settings.setTools).toHaveBeenCalledWith(botId, { helpers: false, computer: false, web: false });
    expect(set.routines.create).toHaveBeenCalledWith(botId, {
      title: 'Morning sort',
      prompt: inbox.routine?.prompt,
      schedule: { kind: 'weekdays', time: '09:00' },
    });
  });

  it('Blank creates a plain bot only: no instructions, tools or routine calls', async () => {
    const { t, set } = transports();
    const blank = STARTERS.find((s) => s.id === 'blank') as StarterTemplate;
    await makeStarterBot(blank, t);
    expect(set.companions.create).toHaveBeenCalledWith(expect.objectContaining({ name: 'Blank' }));
    expect(set.potato.addBot).toHaveBeenCalledTimes(1);
    expect(set.settings.saveInstructions).not.toHaveBeenCalled();
    expect(set.settings.setTools).not.toHaveBeenCalled();
    expect(set.routines.create).not.toHaveBeenCalled();
  });

  it('fails in plain words when no agent is ready, and makes nothing', async () => {
    const companions = fakeCompanionTransport({ agents: [{ id: 'claude', label: 'Claude', ok: false, reason: 'off' }] });
    const create = vi.spyOn(companions, 'create');
    const t: StarterTransports = { companions, potato: fakePotatoTransport({ bots: [] }), settings: fakeBotSettingsTransport(), routines: fakeRoutinesTransport() };
    await expect(makeStarterBot(STARTERS[0], t)).rejects.toThrow('No agent is ready yet.');
    expect(create).not.toHaveBeenCalled();
  });
});

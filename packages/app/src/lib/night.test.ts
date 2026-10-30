// @vitest-environment jsdom
import fs from 'node:fs';
import path from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

const listen = vi.fn();
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...args) }));

import {
  type NightChangedPayload,
  type NightStartInput,
  type NightStateView,
  type NightTaskView,
  type NightView,
  NIGHT_EVENT,
  nightApi,
  nightError,
  nightGet,
  nightErrorCode,
  nightErrorText,
  onNightChanged,
} from './night';

const RUST_NIGHT = path.resolve(__dirname, '../../src-tauri/src/commands/night.rs');

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

beforeEach(() => setTauri(true));

afterEach(() => {
  invoke.mockReset();
  listen.mockReset();
  setTauri(false);
});

const snakeToCamel = (name: string) => name.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());

/**
 * Every `#[tauri::command]` in `commands/night.rs`, with the parameters the page sends (camelCase, as Tauri
 * maps them). `AppHandle` and `State` are filled in by Tauri, so they are left out.
 */
function rustCommands(): Map<string, string[]> {
  const source = fs.readFileSync(RUST_NIGHT, 'utf8');
  const found = new Map<string, string[]>();
  for (const match of source.matchAll(/#\[tauri::command\]\s*pub async fn (\w+)\(([\s\S]*?)\)\s*->/g)) {
    const params = [...match[2].matchAll(/(\w+)\s*:\s*((?:[^,<]|<[^>]*>)+)/g)]
      .filter((param) => !/AppHandle|State</.test(param[2]))
      .map((param) => snakeToCamel(param[1]));
    found.set(match[1], params);
  }
  return found;
}

const startInput: NightStartInput = {
  tasks: [{ project: 'C:/work/app', text: 'Fix the cart total.', who: null, mayChangeTests: false, turns: 12, minutes: 90 }],
  plans: ['codex'],
  stopHhmm: '07:00',
  startHhmm: null,
  parallel: 1,
  keepAwake: false,
};

const settings = { plans: ['codex'], stopHhmm: '07:00', parallel: 1, turns: 12, minutes: 90, awakeAsked: false };

const cases: Array<[string, () => Promise<unknown>]> = [
  ['night_state', () => nightApi.state()],
  ['night_plans', () => nightApi.plans(['C:/work/app'])],
  ['night_checks_get', () => nightApi.checksGet('C:/work/app')],
  ['night_checks_set', () => nightApi.checksSet('C:/work/app', [{ name: 'tests', command: 'npm test', timeoutMin: 10 }])],
  ['night_start', () => nightApi.start(startInput)],
  ['night_stop', () => nightApi.stop()],
  ['night_keep', () => nightApi.keep('nt-00000001')],
  ['night_throw_away', () => nightApi.throwAway('nt-00000001')],
  ['night_keep_all_ready', () => nightApi.keepAllReady()],
  ['night_get', () => nightGet('ns-0123456789ab')],
  ['night_seen', () => nightApi.seen()],
  ['night_report_html', () => nightApi.reportHtml()],
  ['night_report_save', () => nightApi.reportSave('C:/Users/you/Desktop/night.html')],
  ['night_settings_get', () => nightApi.settingsGet()],
  ['night_settings_set', () => nightApi.settingsSet(settings)],
];

describe('nightApi commands', () => {
  it('names every night_* command the Rust side declares, with the argument keys Tauri expects', async () => {
    const rust = rustCommands();
    expect([...rust.keys()].sort()).toEqual(cases.map(([name]) => name).sort());

    for (const [name, run] of cases) {
      invoke.mockReset();
      invoke.mockResolvedValueOnce(undefined);
      await run();
      expect(invoke).toHaveBeenCalledTimes(1);
      const [command, args] = invoke.mock.calls[0] as [string, Record<string, unknown> | undefined];
      expect(command).toBe(name);
      expect(Object.keys(args ?? {}).sort()).toEqual([...(rust.get(name) ?? [])].sort());
    }
  });

  it('sends the task id as taskId and the start sheet as input', async () => {
    invoke.mockResolvedValue(undefined);
    await nightApi.keep('nt-00000001');
    expect(invoke).toHaveBeenLastCalledWith('night_keep', { taskId: 'nt-00000001' });
    await nightApi.start(startInput);
    expect(invoke).toHaveBeenLastCalledWith('night_start', { input: startInput });
    await nightGet('ns-0123456789ab');
    expect(invoke).toHaveBeenLastCalledWith('night_get', { id: 'ns-0123456789ab' });
  });

  it('returns the view the command resolves with, unchanged', async () => {
    const task: NightTaskView = {
      id: 'nt-00000001',
      title: 'Fix the cart total rounding.',
      project: 'app',
      plan: 'Codex',
      state: 'finished',
      stateText: 'Finished',
      until: null,
      class: 'ready',
      headline: 'Tests pass 42 of 42',
      reason: null,
      tests: { passed: 42, total: 42 },
      testsBefore: { passed: 40, total: 42 },
      testChangeLines: [],
      files: { count: 5, added: 120, removed: 34 },
      turns: 3,
      duration: '38 min',
      summaries: ['Fixed the rounding in cart.ts'],
      attempts: [
        { n: 1, label: 'Codex', end: 'completed', sentence: null, started: '23:10', ended: '23:48' },
      ],
      review: 'pending',
      reviewText: null,
      into: null,
      canKeep: true,
      canThrowAway: true,
      paneId: 'pane-42',
    };
    const view: NightView = {
      id: 'ns-0123456789ab',
      rev: 7,
      state: 'done',
      line: '1 of 1 done, tests green.',
      total: 1,
      done: 1,
      green: 1,
      needsYou: 0,
      didntRun: 0,
      noChanges: 0,
      keepAll: 1,
      from: '23:10',
      to: '05:22',
      turns: 3,
      duration: '38 min',
      byPlan: [{ label: 'Codex', tasks: 1 }],
      tasks: [task],
      events: [{ at: '23:10', kind: 'started', glyph: '~', text: 'Started in app', task: null }],
    };
    const state: NightStateView = {
      enabled: true,
      run: view,
      canStart: false,
      startBlocked: "Finish or stop tonight's run first.",
      history: [{ id: 'ns-0123456789ab', line: '1 of 1 done.' }],
      settings,
    };
    invoke.mockResolvedValueOnce(state);
    expect(await nightApi.state()).toEqual(state);
    expect(invoke).toHaveBeenCalledWith('night_state', undefined);
  });

  it('refuses outside the desktop shell and calls nothing', async () => {
    setTauri(false);
    await expect(nightApi.state()).rejects.toThrow('Desktop shell required: night_state');
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe('nightError', () => {
  it('reads an OperationError object as its code and plain message', () => {
    const err = { code: 'RUN_ACTIVE', message: "Finish or stop tonight's run first.", recoverable: true, context: null };
    expect(nightError(err)).toEqual({ code: 'RUN_ACTIVE', message: "Finish or stop tonight's run first." });
    expect(nightErrorText(err)).toBe("Finish or stop tonight's run first.");
    expect(nightErrorCode(err)).toBe('RUN_ACTIVE');
  });

  it('strips the CODE prefix from a CODE: message string', () => {
    expect(nightErrorText('NIGHT_SHIFT_OFF: A person can turn Night Shift on in Settings > General > Labs.')).toBe(
      'A person can turn Night Shift on in Settings > General > Labs.',
    );
    expect(nightErrorCode('NIGHT_SHIFT_OFF: A person can turn Night Shift on.')).toBe('NIGHT_SHIFT_OFF');
  });

  it('strips the CODE prefix from an Error', () => {
    const err = new Error('BAD_TIME: Pick a time like 07:00.');
    expect(nightErrorText(err)).toBe('Pick a time like 07:00.');
    expect(nightErrorCode(err)).toBe('BAD_TIME');
  });

  it('keeps the words of an error that has no code', () => {
    expect(nightErrorText(new Error('Network down'))).toBe('Network down');
    expect(nightErrorText('Something: happened')).toBe('Something: happened');
    expect(nightErrorCode('Something: happened')).toBe('');
  });
});

describe('onNightChanged', () => {
  it('listens to night://changed and passes each payload on', async () => {
    const unlisten = vi.fn();
    let onEvent: (event: { payload: NightChangedPayload }) => void = () => {};
    listen.mockImplementation((_name: string, handler: typeof onEvent) => {
      onEvent = handler;
      return Promise.resolve(unlisten);
    });

    const seen: NightChangedPayload[] = [];
    const stop = await onNightChanged((payload) => seen.push(payload));
    expect(listen).toHaveBeenCalledWith(NIGHT_EVENT, expect.any(Function));
    expect(NIGHT_EVENT).toBe('night://changed');

    onEvent({ payload: { rev: 3, state: 'running' } });
    onEvent({ payload: { rev: 4, state: { scheduled: { start_at_ms: 1_000 } } } });
    expect(seen).toEqual([
      { rev: 3, state: 'running' },
      { rev: 4, state: { scheduled: { start_at_ms: 1_000 } } },
    ]);

    stop();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it('does nothing outside the desktop shell', async () => {
    setTauri(false);
    const stop = await onNightChanged(() => {});
    expect(listen).not.toHaveBeenCalled();
    expect(() => stop()).not.toThrow();
  });
});

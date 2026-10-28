// @vitest-environment jsdom
// Tonight (SPEC n32 3.10): Start is off with one line that says why; ticking a second login of one agent unticks the
// first; the keep-awake confirm shows once; suggestions are never pre-selected and Use saves; the worst-case line adds
// up; tasks reorder with the arrows; ten tasks is the cap. `invoke` is mocked, so the real `lib/night.ts` runs.
import fs from 'node:fs';
import path from 'node:path';
import { act, type ComponentProps } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  LABS_OFF_SENTENCE,
  type NightChecksView,
  type NightCheckInput,
  type NightPlanView,
  type NightSettingsView,
  type NightStateView,
  type NightView,
} from '../../../lib/night';
import { TonightSheet, type TonightProject } from './TonightSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: () => Promise.resolve(() => {}) }));

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

/** 28 Oct 2026, 20:00 on this computer. A 07:00 stop is eleven hours on, so the default is valid. */
const NOW = new Date(2026, 9, 28, 20, 0).getTime();

const PROJECTS: TonightProject[] = [
  { path: 'C:/work/app', name: 'app', git: true },
  { path: 'C:/work/docs-site', name: 'docs-site', git: true },
  { path: 'C:/work/scratch', name: 'scratch', git: false },
];

const SETTINGS: NightSettingsView = {
  plans: ['codex'],
  stopHhmm: '07:00',
  parallel: 2,
  turns: 12,
  minutes: 90,
  awakeAsked: false,
};

const state = (over: Partial<NightStateView> = {}): NightStateView => ({
  enabled: true,
  run: null,
  canStart: true,
  startBlocked: null,
  history: [],
  settings: SETTINGS,
  ...over,
});

const PLANS: NightPlanView[] = [
  { key: 'codex', label: 'Codex', ready: true, whyNot: null, outUntil: null, triedHere: { 'C:/work/app': true } },
  {
    key: 'claude',
    label: 'Claude Code',
    ready: true,
    whyNot: null,
    outUntil: 'Out of usage until 16:40',
    triedHere: {},
  },
];

const RUN = { id: 'nr-1', state: 'scheduled' } as unknown as NightView;

/** The checks of each folder. Saves change the store, the way the desktop does. */
let store: Record<string, NightChecksView>;
function resetStore() {
  store = {
    'C:/work/app': {
      declared: [{ name: 'npm test', argv: ['npm', 'test'], timeoutMin: 10 }],
      suggestions: [],
      warnings: [],
      baselineRed: null,
    },
    'C:/work/docs-site': {
      declared: [],
      suggestions: [{ name: 'lint', argv: ['npm', 'run', 'lint'], source: 'package.json' }],
      warnings: [],
      baselineRed: null,
    },
    'C:/work/scratch': { declared: [], suggestions: [], warnings: [], baselineRed: null },
  };
}

interface Answers {
  state?: NightStateView;
  plans?: NightPlanView[];
  start?: () => Promise<NightView>;
}

/** Answers every `night_*` command the sheet sends, from the store above. */
function answer(over: Answers = {}) {
  invoke.mockImplementation(async (command: string, args?: Record<string, any>) => {
    switch (command) {
      case 'night_state':
        return over.state ?? state();
      case 'night_plans':
        return over.plans ?? PLANS;
      case 'night_checks_get':
        return store[args?.project as string];
      case 'night_checks_set': {
        const list = args?.list as NightCheckInput[];
        const project = args?.project as string;
        store[project] = {
          ...store[project],
          declared: list.map((check) => ({ name: check.name, argv: check.command.split(' '), timeoutMin: check.timeoutMin })),
        };
        return store[project].declared;
      }
      case 'night_start':
        return over.start ? over.start() : RUN;
      case 'night_settings_set':
        return args?.settings;
      default:
        throw new Error(`unexpected command ${command}`);
    }
  });
}

const callsTo = (command: string) => invoke.mock.calls.filter(([name]) => name === command);

let host: HTMLDivElement;
let root: Root;

beforeEach(() => {
  setTauri(true);
  resetStore();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  invoke.mockReset();
  setTauri(false);
});

/** Lets the reads the sheet starts with finish. */
async function flush() {
  for (let i = 0; i < 5; i += 1) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function mount(props: Partial<ComponentProps<typeof TonightSheet>> = {}) {
  const onClose = vi.fn();
  const onNotify = vi.fn();
  const onStarted = vi.fn();
  await act(async () => {
    root.render(
      <TonightSheet
        projects={PROJECTS}
        now={() => NOW}
        onClose={onClose}
        onNotify={onNotify}
        onStarted={onStarted}
        {...props}
      />,
    );
  });
  await flush();
  return { onClose, onNotify, onStarted };
}

function labelled(label: string): HTMLElement {
  const found = host.querySelector<HTMLElement>(`[aria-label="${label}"]`);
  if (!found) throw new Error(`no element labelled "${label}"`);
  return found;
}

function button(text: string): HTMLButtonElement {
  const found = [...host.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
  if (!found) throw new Error(`no button "${text}"`);
  return found;
}

const startButton = () => button('Start Night Shift');
const blockedLine = () => host.querySelector('.ck-night-blocked')?.textContent ?? null;
const taskEl = (n: number) => host.querySelectorAll<HTMLElement>('.ck-night-task')[n - 1];
/** The "More" toggle of a task (the Tests chip is also a button with aria-expanded, so it is found by its words). */
const moreOf = (n: number) =>
  [...taskEl(n).querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.startsWith('More')) as HTMLButtonElement;
const planBox = (name: string) => {
  const row = [...host.querySelectorAll<HTMLLabelElement>('label.ck-night-plan')].find((l) =>
    l.textContent?.includes(name),
  );
  if (!row) throw new Error(`no plan "${name}"`);
  return row.querySelector<HTMLInputElement>('input') as HTMLInputElement;
};

async function click(el: Element) {
  await act(async () => {
    (el as HTMLElement).click();
  });
  await flush();
}

/** Sets a field the way a person types or picks, so React's change handlers run. */
async function type(el: HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement, value: string) {
  const proto =
    el instanceof HTMLSelectElement
      ? HTMLSelectElement.prototype
      : el instanceof HTMLTextAreaElement
        ? HTMLTextAreaElement.prototype
        : HTMLInputElement.prototype;
  const setValue = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
  if (!setValue) throw new Error('no value setter');
  await act(async () => {
    setValue.call(el, value);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  });
  await flush();
}

/** A task with its words filled in, so that only the thing under test can block Start. */
async function writeTask(text = 'Fix the cart total rounding.') {
  await type(labelled('Task 1') as HTMLTextAreaElement, text);
}

describe('TonightSheet', () => {
  describe('Start is off, and one line says why', () => {
    it('is on for a complete draft', async () => {
      answer();
      await mount();
      await writeTask();
      expect(startButton().disabled).toBe(false);
      expect(blockedLine()).toBeNull();
    });

    it('says to add a task when there are none', async () => {
      answer();
      await mount();
      await click(labelled('Remove task 1'));
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Add at least one task.');
    });

    it('says what is missing when a task has no words', async () => {
      answer();
      await mount();
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Task 1 needs words: what should it do?');
    });

    it('says a folder must be a git project with a commit', async () => {
      answer();
      await mount();
      await writeTask();
      await type(labelled('Project for task 1') as HTMLSelectElement, 'C:/work/scratch');
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe(
        'Task 1: this project is not a git project with a commit. Night Shift works in a separate copy of it.',
      );
    });

    it('says Undo must be on', async () => {
      answer();
      await mount({ undoOn: false });
      await writeTask();
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe(
        'Missions need Undo to go back when a turn makes things worse. Undo is off for this folder.',
      );
    });

    it('says a plan must be ticked', async () => {
      answer();
      await mount();
      await writeTask();
      await click(planBox('Codex'));
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Tick at least one plan to use.');
    });

    it('says the stop time is too soon', async () => {
      answer();
      await mount();
      await writeTask();
      await type(labelled('Stop at') as HTMLInputElement, '20:20');
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Pick a stop time at least 30 minutes after the start.');
    });

    it('says the stop time is too far away', async () => {
      answer();
      await mount();
      await writeTask();
      await type(labelled('Stop at') as HTMLInputElement, '19:00');
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Pick a stop time within 14 hours of the start.');
    });

    it('says a time is missing', async () => {
      answer();
      await mount();
      await writeTask();
      await type(labelled('Stop at') as HTMLInputElement, '');
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Pick a time like 07:00.');
    });

    it('says the budget is out of range', async () => {
      answer();
      await mount();
      await writeTask();
      await click(moreOf(1));
      await type(taskEl(1).querySelector('input[type="number"]') as HTMLInputElement, '2');
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe('Task 1: turns must be 3 to 40, and minutes 20 to 240.');
    });

    it('says an unfinished run must end first', async () => {
      answer({ state: state({ run: RUN }) });
      await mount();
      await writeTask();
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe("Finish or stop tonight's run first.");
    });

    it('says Labs must be on', async () => {
      answer({ state: state({ enabled: false }) });
      await mount();
      await writeTask();
      expect(startButton().disabled).toBe(true);
      expect(blockedLine()).toBe(LABS_OFF_SENTENCE);
    });

    it('sends the tasks and the times, closes, and makes the toast', async () => {
      answer();
      const { onClose, onNotify, onStarted } = await mount();
      await writeTask();
      await click(startButton());

      expect(invoke).toHaveBeenCalledWith('night_start', {
        input: {
          tasks: [
            {
              project: 'C:/work/app',
              text: 'Fix the cart total rounding.',
              who: null,
              mayChangeTests: false,
              turns: 12,
              minutes: 90,
            },
          ],
          plans: ['codex'],
          stopHhmm: '07:00',
          startHhmm: null,
          parallel: 2,
          keepAwake: false,
        },
      });
      expect(onStarted).toHaveBeenCalledWith(RUN);
      expect(onNotify).toHaveBeenCalledWith('Night Shift started. It stops at 07:00.');
      expect(onClose).toHaveBeenCalled();
    });

    it('shows the desktop refusal when Start fails, and keeps the sheet open', async () => {
      const refusal = {
        code: 'PLAN_SAME_COMPANY',
        message: "Puppet Master won't switch between two logins of one company. Untick one.",
        recoverable: true,
        context: null,
      };
      answer({ start: () => Promise.reject(refusal) });
      const { onClose } = await mount();
      await writeTask();
      await click(startButton());

      expect(host.querySelector('[role="alert"]')?.textContent).toBe(refusal.message);
      expect(onClose).not.toHaveBeenCalled();
      expect(startButton().disabled).toBe(false);
    });

    it('starts at a set time and says it is tomorrow when the stop falls on the next day', async () => {
      answer();
      await mount();
      await writeTask();
      const radios = host.querySelectorAll<HTMLInputElement>('input[name="ck-night-start"]');
      await click(radios[1]);
      await type(labelled('Start at') as HTMLInputElement, '23:30');
      expect(host.querySelector('.ck-night-when')?.textContent).toContain('tomorrow');
      await click(startButton());
      expect(invoke).toHaveBeenCalledWith(
        'night_start',
        expect.objectContaining({ input: expect.objectContaining({ startHhmm: '23:30' }) }),
      );
    });
  });

  describe('plans', () => {
    it('unticks the first login when a second login of one agent is ticked', async () => {
      answer({
        plans: [
          ...PLANS,
          { key: 'codex@work', label: 'Codex · Work', ready: true, whyNot: null, outUntil: null, triedHere: {} },
        ],
      });
      await mount();
      expect(planBox('Codex').checked).toBe(true);
      await click(planBox('Codex · Work'));

      expect(planBox('Codex').checked).toBe(false);
      expect(planBox('Codex · Work').checked).toBe(true);
      expect(host.querySelector('[role="status"]')?.textContent).toBe(
        "Puppet Master won't switch between two logins of one company.",
      );
    });

    it('keeps an out-of-usage plan enabled and shows the time', async () => {
      answer();
      await mount();
      expect(planBox('Claude Code').disabled).toBe(false);
      expect(host.textContent).toContain('Out of usage until 16:40');
    });

    it('lists a plan that cannot run with its reason and leaves it unticked', async () => {
      answer({
        plans: [{ key: 'cursor', label: 'Cursor', ready: false, whyNot: 'Not set up here.', outUntil: null, triedHere: {} }],
      });
      await mount();
      expect(planBox('Cursor').disabled).toBe(true);
      expect(host.textContent).toContain('Not set up here.');
    });
  });

  describe('keep awake', () => {
    const awakeSwitch = () =>
      labelled('Keep this computer awake until it is done') as HTMLButtonElement;
    const WORDS = 'Puppet Master will stop this computer from sleeping until Night Shift ends';

    it('asks once, saves that it was asked, and turns the switch on for "Keep it awake"', async () => {
      answer();
      await mount();
      await click(awakeSwitch());
      expect(host.textContent).toContain(WORDS);

      await click(button('Keep it awake'));
      expect(callsTo('night_settings_set')).toHaveLength(1);
      expect(invoke).toHaveBeenCalledWith(
        'night_settings_set',
        expect.objectContaining({ settings: expect.objectContaining({ awakeAsked: true }) }),
      );
      expect(awakeSwitch().getAttribute('aria-checked')).toBe('true');
      expect(host.textContent).not.toContain(WORDS);
    });

    it('does not ask again after the answer', async () => {
      answer();
      await mount();
      await click(awakeSwitch());
      await click(button('Keep it awake'));
      await click(awakeSwitch());
      expect(awakeSwitch().getAttribute('aria-checked')).toBe('false');
      await click(awakeSwitch());
      expect(awakeSwitch().getAttribute('aria-checked')).toBe('true');
      expect(host.textContent).not.toContain(WORDS);
      expect(callsTo('night_settings_set')).toHaveLength(1);
    });

    it('saves the answer "Don\'t" and leaves the switch off', async () => {
      answer();
      await mount();
      await click(awakeSwitch());
      await click(button("Don't"));
      expect(awakeSwitch().getAttribute('aria-checked')).toBe('false');
      expect(callsTo('night_settings_set')).toHaveLength(1);
    });

    it('does not ask at all when the person already answered', async () => {
      answer({ state: state({ settings: { ...SETTINGS, awakeAsked: true } }) });
      await mount();
      await click(awakeSwitch());
      expect(host.textContent).not.toContain(WORDS);
      expect(awakeSwitch().getAttribute('aria-checked')).toBe('true');
    });
  });

  describe('tests', () => {
    it('never pre-selects a suggestion', async () => {
      answer();
      await mount();
      await type(labelled('Project for task 1') as HTMLSelectElement, 'C:/work/docs-site');
      await click(labelled('Tests for task 1'));

      expect(host.textContent).toContain('Found: lint in package.json');
      expect(callsTo('night_checks_set')).toHaveLength(0);
      expect(labelled('Tests for task 1').textContent).toBe('Pick tests…');
    });

    it('saves a suggestion only when Use is pressed', async () => {
      answer();
      await mount();
      await type(labelled('Project for task 1') as HTMLSelectElement, 'C:/work/docs-site');
      await click(labelled('Tests for task 1'));
      await click(button('Use'));

      expect(invoke).toHaveBeenCalledWith('night_checks_set', {
        project: 'C:/work/docs-site',
        list: [{ name: 'lint', command: 'npm run lint', timeoutMin: 10 }],
      });
      expect(labelled('Tests for task 1').textContent).toBe('lint');
    });

    it('adds a typed command next to the declared ones', async () => {
      answer();
      await mount();
      await click(labelled('Tests for task 1'));
      await type(labelled('Add a test command') as HTMLInputElement, 'npm run build');
      await click(button('Add'));

      expect(invoke).toHaveBeenCalledWith('night_checks_set', {
        project: 'C:/work/app',
        list: [
          { name: 'npm test', command: 'npm test', timeoutMin: 10 },
          { name: 'npm run build', command: 'npm run build', timeoutMin: 10 },
        ],
      });
    });

    it('shows the "No tests" choice on the chip', async () => {
      answer();
      await mount();
      await type(labelled('Project for task 1') as HTMLSelectElement, 'C:/work/scratch');
      await click(labelled('Tests for task 1'));
      await click(button('No tests for this project'));
      expect(labelled('Tests for task 1').textContent).toBe('No tests');
      expect(host.textContent).toContain('No tests declared. It will run, but nothing will be checked.');
    });

    it('says who has worked in the folder before, and that a first time may need a trust', async () => {
      answer({
        state: state({ settings: { ...SETTINGS, plans: ['codex', 'claude'] } }),
      });
      await mount();
      expect(host.textContent).toContain('Codex has worked here before.');

      await type(labelled('Agent for task 1') as HTMLSelectElement, 'claude');
      expect(host.textContent).toContain('First time with Claude Code here: it may ask you to trust the folder.');
    });
  });

  describe('the worst-case line and the tasks', () => {
    it('adds up three tasks at twelve turns each', async () => {
      answer();
      await mount();
      await click(button('+ Add a task (1 of 10)'));
      await click(button('+ Add a task (2 of 10)'));
      expect(host.textContent).toContain('Worst case: 3 tasks x 12 turns = 36 turns, at most 2 at a time, until 07:00.');
    });

    it('adds up tasks with different turns', async () => {
      answer();
      await mount();
      await click(button('+ Add a task (1 of 10)'));
      await click(button('+ Add a task (2 of 10)'));
      await click(moreOf(2));
      await type(taskEl(2).querySelector('input[type="number"]') as HTMLInputElement, '5');
      expect(host.textContent).toContain('Worst case: 3 tasks, 29 turns in all, at most 2 at a time, until 07:00.');
    });

    it('moves a task up and down with its arrows', async () => {
      answer();
      await mount();
      await click(button('+ Add a task (1 of 10)'));
      await click(button('+ Add a task (2 of 10)'));
      await type(labelled('Task 1') as HTMLTextAreaElement, 'A');
      await type(labelled('Task 2') as HTMLTextAreaElement, 'B');
      await type(labelled('Task 3') as HTMLTextAreaElement, 'C');
      await click(labelled('Move task 3 up'));

      const texts = [...host.querySelectorAll('textarea')].map((t) => t.value);
      expect(texts).toEqual(['A', 'C', 'B']);
      expect(labelled('Move task 1 up').hasAttribute('disabled')).toBe(true);
    });

    it('stops at ten tasks', async () => {
      answer();
      await mount();
      for (let n = 2; n <= 10; n += 1) {
        await click(host.querySelector<HTMLButtonElement>('.ck-night-add') as HTMLButtonElement);
      }
      expect(host.querySelectorAll('.ck-night-task')).toHaveLength(10);
      const add = host.querySelector<HTMLButtonElement>('.ck-night-add') as HTMLButtonElement;
      expect(add.textContent).toBe('+ Add a task (10 of 10)');
      expect(add.disabled).toBe(true);
    });
  });

  it('keeps night.css to grayscale tokens only', () => {
    const css = fs.readFileSync(path.resolve(__dirname, '../../../styles/night.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
    expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
    expect(css).not.toMatch(/\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/i);
  });
});

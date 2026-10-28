// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { CONFLICT_STATE, EMPTY_STATE, MORNING_4_OF_5, MORNING_STATE, RUNNING_STATE } from '../../../dev/night-fixtures';
import { type NightStateView, NIGHT_EVENT, type NightTaskView, type NightView } from '../../../lib/night';
import { EMPTY_TEXT, MorningPage, groupTasks, keepAllNotice, passingTasks } from './MorningPage';
import { CONFIRM_MS, tryText } from './TaskRow';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const invoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...args: unknown[]) => invoke(...args) }));

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();
const listen = vi.fn((name: string, handler: Handler) => {
  handlers.set(name, handler);
  return Promise.resolve(() => {
    handlers.delete(name);
  });
});
vi.mock('@tauri-apps/api/event', () => ({ listen: (...args: unknown[]) => listen(...(args as [string, Handler])) }));

const setTauri = (on: boolean) => {
  const w = window as unknown as Record<string, unknown>;
  if (on) w.__TAURI_INTERNALS__ = {};
  else delete w.__TAURI_INTERNALS__;
};

// What each `night_*` command answers in a test. A command with no answer fails the test.
type Reply = (args?: Record<string, unknown>) => unknown;
let replies: Record<string, Reply> = {};

const TITLE_READY = 'Fix the cart total rounding.';
const TITLE_RENAME = 'Rename the config loader.';
const TITLE_KEPT = 'Add the export button.';
const TITLE_UPDATE = 'Update the account page copy.';
const TITLE_LOOK = 'Fix broken links in /guides';
const TITLE_CONFLICT = 'Tidy the cart helpers.';

let host: HTMLDivElement;
let root: Root;

/** Lets the pending reads and answers settle. */
async function flush() {
  for (let i = 0; i < 20; i += 1) {
    await act(async () => {
      await Promise.resolve();
    });
  }
}

async function open(state: NightStateView) {
  replies.night_state = () => state;
  await act(async () => {
    root.render(<MorningPage onClose={() => {}} />);
  });
  await flush();
}

async function press(el: HTMLElement) {
  await act(async () => {
    el.click();
  });
  await flush();
}

const calls = (cmd: string) => invoke.mock.calls.filter(([name]) => name === cmd);
const rows = () => Array.from(host.querySelectorAll<HTMLElement>('li.ck-morning-task'));

function rowOf(title: string): HTMLElement {
  const row = rows().find((li) => li.querySelector('.ck-morning-title')?.textContent === title);
  if (!row) throw new Error(`no row for "${title}"`);
  return row;
}

function button(scope: ParentNode, label: string): HTMLButtonElement {
  const found = Array.from(scope.querySelectorAll<HTMLButtonElement>('button')).filter(
    (b) => b.textContent?.trim() === label,
  );
  if (found.length !== 1) throw new Error(`expected one "${label}" button, found ${found.length}`);
  return found[0];
}

function labels(scope: ParentNode): string[] {
  return Array.from(scope.querySelectorAll('button')).map((b) => b.textContent?.trim() ?? '');
}

function headings(): string[] {
  return Array.from(host.querySelectorAll('.ck-morning-group-title')).map((h) => h.textContent ?? '');
}

/** A copy of the night with one task changed (the `rev` moves on, as the backend's does). */
function withTask(view: NightView, id: string, change: Partial<NightTaskView>, rev = view.rev + 1): NightView {
  return { ...view, rev, tasks: view.tasks.map((t) => (t.id === id ? { ...t, ...change } : t)) };
}

beforeEach(() => {
  setTauri(true);
  replies = { night_seen: () => null };
  invoke.mockImplementation(async (cmd: string, args?: Record<string, unknown>) => {
    const reply = replies[cmd];
    if (!reply) throw new Error(`no reply for ${cmd}`);
    return reply(args);
  });
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
  invoke.mockReset();
  listen.mockClear();
  handlers.clear();
  setTauri(false);
});

describe('MorningPage grouping and counts', () => {
  it('sorts the tasks into groups with their counts', async () => {
    await open(MORNING_STATE);
    expect(headings()).toEqual(['READY TO KEEP (4)', 'NEEDS A LOOK (1)', 'THE NIGHT']);
    expect(rows()).toHaveLength(5);
    expect(rowOf(TITLE_LOOK).closest('section')?.querySelector('h2')?.textContent).toBe('NEEDS A LOOK (1)');
  });

  it('keeps the groups in order and leaves empty ones out', () => {
    const base = MORNING_4_OF_5.tasks[0];
    const tasks: NightTaskView[] = [
      { ...base, id: 'a', class: 'didnt_run', headline: "Didn't run: no time left before 07:00." },
      { ...base, id: 'b', class: 'no_changes', headline: 'Made no changes.' },
      { ...base, id: 'c', state: 'working', class: null, headline: null },
      { ...base, id: 'd', class: 'ready' },
    ];
    expect(groupTasks(tasks).map((g) => `${g.title} ${g.tasks.length}`)).toEqual([
      'READY TO KEEP 1',
      "DIDN'T RUN 1",
      'MADE NO CHANGES 1',
      'IN PROGRESS 1',
    ]);
  });

  it('counts only finished Ready copies that nobody has kept yet for Keep all', () => {
    expect(passingTasks(MORNING_4_OF_5.tasks).map((t) => t.title)).toEqual([
      TITLE_READY,
      TITLE_RENAME,
      TITLE_UPDATE,
    ]);
  });

  it('shows the Keep all count from those copies', async () => {
    await open(MORNING_STATE);
    expect(button(host, 'Keep all that pass (3)').disabled).toBe(false);
  });
});

describe('MorningPage Keep and Throw away', () => {
  it('needs a second press to keep, and keeps then', async () => {
    replies.night_keep = () => withTask(MORNING_4_OF_5, 'nt-00000001', { review: 'kept', reviewText: 'Kept · merged into main', into: 'main', canKeep: false, canThrowAway: false });
    await open(MORNING_STATE);

    await press(button(rowOf(TITLE_READY), 'Keep'));
    expect(calls('night_keep')).toHaveLength(0);
    expect(rowOf(TITLE_READY).textContent).toContain('Merge this copy?');

    await press(button(rowOf(TITLE_READY), 'Keep'));
    expect(calls('night_keep')).toEqual([['night_keep', { taskId: 'nt-00000001' }]]);
    expect(rowOf(TITLE_READY).textContent).toContain('Kept · merged into main');
  });

  it('a Keep question goes back to the button after 3 s with no keep', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    await open(MORNING_STATE);

    await press(button(rowOf(TITLE_READY), 'Keep'));
    expect(rowOf(TITLE_READY).textContent).toContain('Merge this copy?');

    await act(async () => {
      vi.advanceTimersByTime(CONFIRM_MS);
    });
    expect(rowOf(TITLE_READY).textContent).not.toContain('Merge this copy?');
    expect(button(rowOf(TITLE_READY), 'Keep')).toBeTruthy();
    expect(calls('night_keep')).toHaveLength(0);
  });

  it('a NeedsLook Keep asks with its reason and does nothing on the first press', async () => {
    await open(MORNING_STATE);

    await press(button(rowOf(TITLE_LOOK), 'Keep'));
    const text = rowOf(TITLE_LOOK).textContent ?? '';
    expect(text).toContain('Look at the failing tests before you keep it.');
    expect(text).toContain('Keep anyway?');
    expect(calls('night_keep')).toHaveLength(0);
  });

  it('a Throw away needs a second press, and removes the copy then', async () => {
    replies.night_throw_away = () => withTask(MORNING_4_OF_5, 'nt-00000002', { review: 'discarded', reviewText: 'Thrown away', canKeep: false, canThrowAway: false });
    await open(MORNING_STATE);

    await press(button(rowOf(TITLE_RENAME), 'Throw away'));
    expect(calls('night_throw_away')).toHaveLength(0);
    expect(rowOf(TITLE_RENAME).textContent).toContain('Delete this copy?');

    await press(button(rowOf(TITLE_RENAME), 'Throw away'));
    expect(calls('night_throw_away')).toEqual([['night_throw_away', { taskId: 'nt-00000002' }]]);
    expect(rowOf(TITLE_RENAME).textContent).toContain('Thrown away');
  });

  it('a Throw away question goes back to the button after 3 s', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    await open(MORNING_STATE);

    await press(button(rowOf(TITLE_RENAME), 'Throw away'));
    expect(rowOf(TITLE_RENAME).textContent).toContain('Delete this copy?');

    await act(async () => {
      vi.advanceTimersByTime(CONFIRM_MS);
    });
    expect(rowOf(TITLE_RENAME).textContent).not.toContain('Delete this copy?');
    expect(calls('night_throw_away')).toHaveLength(0);
  });

  it('a kept task is only a line with no buttons', async () => {
    await open(MORNING_STATE);
    const row = rowOf(TITLE_KEPT);
    expect(row.textContent).toContain('Kept · merged into main');
    expect(labels(row)).toEqual([]);
  });

  it('shows a refusal in plain words on the row', async () => {
    replies.night_keep = () => {
      throw { code: 'RUN_ACTIVE', message: "Finish or stop tonight's run first.", recoverable: false, context: null };
    };
    await open(MORNING_STATE);

    await press(button(rowOf(TITLE_READY), 'Keep'));
    await press(button(rowOf(TITLE_READY), 'Keep'));
    const alert = rowOf(TITLE_READY).querySelector('[role="alert"]');
    expect(alert?.textContent).toBe("Finish or stop tonight's run first.");
  });
});

describe('MorningPage conflicts', () => {
  it('a conflict row shows its text and offers Throw away, not Keep', async () => {
    await open(CONFLICT_STATE);
    const row = rowOf(TITLE_CONFLICT);
    expect(row.textContent).toContain('This conflicts with a task you already kept (src/cart.ts).');
    expect(labels(row)).toContain('Throw away');
    expect(labels(row)).not.toContain('Keep');
  });
});

describe('MorningPage Keep all that pass', () => {
  it('lists the titles, then keeps in order and stops after a conflict', async () => {
    replies.night_keep_all_ready = () => {
      const kept = withTask(MORNING_4_OF_5, 'nt-00000001', { review: 'kept', reviewText: 'Kept · merged into main', into: 'main', canKeep: false });
      return withTask(kept, 'nt-00000002', {
        review: 'keep_failed',
        reviewText: 'This conflicts with a task you already kept (src/cart.ts). Throw it away, or merge it yourself.',
        canKeep: false,
        canThrowAway: false,
      });
    };
    await open(MORNING_STATE);

    await press(button(host, 'Keep all that pass (3)'));
    const ask = host.querySelector('.ck-morning-ask-all');
    expect(Array.from(ask?.querySelectorAll('li') ?? []).map((li) => li.textContent)).toEqual([
      TITLE_READY,
      TITLE_RENAME,
      TITLE_UPDATE,
    ]);
    expect(calls('night_keep_all_ready')).toHaveLength(0);

    await press(button(host, 'Keep all'));
    expect(calls('night_keep_all_ready')).toHaveLength(1);

    const notice = host.querySelector('.ck-morning-notice');
    expect(notice?.querySelector('p')?.textContent).toBe('Kept 1 of 3. Stopped at a conflict. Still to keep:');
    expect(Array.from(notice?.querySelectorAll('li') ?? []).map((li) => li.textContent)).toEqual([TITLE_UPDATE]);
    expect(rowOf(TITLE_RENAME).textContent).toContain('This conflicts with a task you already kept');
  });

  it('Keep all asks again after 3 s when it is not pressed', async () => {
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    await open(MORNING_STATE);

    await press(button(host, 'Keep all that pass (3)'));
    expect(host.querySelector('.ck-morning-ask-all')).not.toBeNull();

    await act(async () => {
      vi.advanceTimersByTime(CONFIRM_MS);
    });
    expect(host.querySelector('.ck-morning-ask-all')).toBeNull();
    expect(calls('night_keep_all_ready')).toHaveLength(0);
  });

  it('keepAllNotice reads the answer in list order', () => {
    const before = passingTasks(MORNING_4_OF_5.tasks);
    const after = withTask(withTask(MORNING_4_OF_5, 'nt-00000001', { review: 'kept' }), 'nt-00000002', { review: 'kept' }).tasks;
    expect(keepAllNotice(before, after)).toEqual({ text: 'Kept 2 of 3. Still to keep:', titles: [TITLE_UPDATE] });
  });
});

describe('MorningPage night_seen', () => {
  it('is called once when the page opens, and not again when the night changes', async () => {
    await open(MORNING_STATE);
    expect(calls('night_seen')).toHaveLength(1);

    const onChanged = handlers.get(NIGHT_EVENT);
    expect(onChanged).toBeDefined();
    await act(async () => {
      onChanged?.({ payload: { rev: 2, state: 'done' } });
    });
    await flush();

    expect(calls('night_state').length).toBeGreaterThanOrEqual(2);
    expect(calls('night_seen')).toHaveLength(1);
  });
});

describe('MorningPage text from the agent', () => {
  it('shows a headline, a title and a turn sentence as text, never as HTML', async () => {
    const hostile = MORNING_4_OF_5.tasks[0];
    const view: NightView = {
      ...MORNING_4_OF_5,
      tasks: [
        {
          ...hostile,
          title: '<img src="x" onerror="window.__pwned=1">',
          headline: '<b>Tests pass</b>',
          summaries: ['<script>window.__pwned=1</script>'],
        },
      ],
    };
    await open({ ...MORNING_STATE, run: view });

    expect(host.querySelector('img')).toBeNull();
    expect(host.querySelector('b')).toBeNull();
    expect(host.querySelector('script')).toBeNull();
    expect(host.textContent).toContain('<img src="x" onerror="window.__pwned=1">');
    expect(host.textContent).toContain('<b>Tests pass</b>');

    await press(host.querySelector<HTMLElement>('[aria-label="Show details"]') as HTMLElement);
    expect(host.textContent).toContain('<script>window.__pwned=1</script>');
    expect(host.querySelector('script')).toBeNull();
  });

  it('shows the tries of a task in the details', async () => {
    await open(MORNING_STATE);
    await press(rowOf(TITLE_READY).querySelector('[aria-label="Show details"]') as HTMLElement);
    expect(rowOf(TITLE_READY).textContent).toContain('Fixed the rounding in cart.ts');
    expect(rowOf(TITLE_READY).textContent).toContain('Try 1 · Codex · finished 23:48');
    expect(tryText({ n: 2, label: 'Codex', end: 'gate', sentence: 'Codex asked to trust the folder.', started: '01:20', ended: null })).toBe(
      'Try 2 · Codex · Codex asked to trust the folder.',
    );
  });
});

describe('MorningPage states', () => {
  it('an empty night says there is no Night Shift yet', async () => {
    await open(EMPTY_STATE);
    expect(host.textContent).toContain(EMPTY_TEXT);
    expect(labels(host).some((label) => label.startsWith('Keep all'))).toBe(false);
  });

  it('a running night lists its tasks and offers no Keep or Throw away', async () => {
    await open(RUNNING_STATE);
    expect(headings()).toEqual(['IN PROGRESS (3)']);
    expect(host.textContent).toContain('Running. Each task shows here as it finishes.');
    expect(labels(host)).not.toContain('Keep');
    expect(labels(host)).not.toContain('Throw away');
  });

  it('a read error is shown in plain words', async () => {
    replies.night_state = () => {
      throw { code: 'NIGHT_READ', message: 'Night Shift could not be read.', recoverable: true, context: null };
    };
    await act(async () => {
      root.render(<MorningPage onClose={() => {}} />);
    });
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Night Shift could not be read.');
  });
});

describe('MorningPage history switcher', () => {
  function choose(value: string) {
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Night"]');
    if (!select) throw new Error('no switcher');
    return act(async () => {
      select.value = value;
      select.dispatchEvent(new Event('change', { bubbles: true }));
    });
  }

  it('shows an older night as its summary and the latest night again', async () => {
    await open(MORNING_STATE);
    const select = host.querySelector<HTMLSelectElement>('select[aria-label="Night"]');
    expect(Array.from(select?.options ?? []).map((o) => o.textContent)).toEqual([
      'Latest night',
      '2 of 2 done, tests green.',
    ]);

    await choose('ns-aaaaaaaaaaaa');
    await flush();
    expect(host.textContent).toContain('2 of 2 done, tests green.');
    expect(host.textContent).toContain('Only the summary of an older night is shown here.');
    expect(headings()).toEqual([]);

    await choose('latest');
    await flush();
    expect(headings()).toEqual(['READY TO KEEP (4)', 'NEEDS A LOOK (1)', 'THE NIGHT']);
  });
});

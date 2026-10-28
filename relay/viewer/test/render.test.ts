import { describe, expect, it } from 'vitest';
import {
  applyFrame,
  durationLabel,
  emptyModel,
  endModel,
  MAX_ITEMS_PER_PANE,
  noticeFor,
  normalizeItem,
  resultLabel,
  stateWord,
  toolMeta,
  type Model,
} from '../src/render';
import type { Frame, ItemView, PaneView } from '../src/types';

const p1: PaneView = { key: 'p1', label: 'Agent A', title: 'Fix the login form', state: 'working', line: 'Running npm test' };
const p2: PaneView = { key: 'p2', label: 'Agent B', title: 'Count lines', state: 'idle', line: null };

function user(id: number, pane: string, text: string): ItemView {
  return { t: 'user', id, pane, text };
}

describe('applyFrame', () => {
  it('snapshot, then delta, then state: items add up and the room and panes follow', () => {
    const model: Model = emptyModel();

    const snap: Frame = {
      v: 1,
      kind: 'snap',
      room: { title: 'Live Room', endsAt: 1000, paused: false, hidden: 0 },
      panes: [p1, p2],
      items: [user(1, 'p1', 'Fix the login bug')],
    };
    applyFrame(model, snap);
    expect(model.status).toBe('live');
    expect(model.panes.map((pane) => pane.key)).toEqual(['p1', 'p2']);
    expect(model.items).toHaveLength(1);

    const delta: Frame = {
      v: 1,
      kind: 'delta',
      panes: [{ ...p1, state: 'idle', line: null }],
      items: [{ t: 'tool', id: 2, pane: 'p1', name: 'Bash', summary: 'npm test', files: 0, writes: false }],
    };
    applyFrame(model, delta);
    expect(model.items).toHaveLength(2);
    expect(model.panes[0]?.state).toBe('idle');
    expect(model.panes[1]?.key).toBe('p2');

    const state: Frame = {
      v: 1,
      kind: 'state',
      room: { title: 'Live Room', endsAt: 1000, paused: true, hidden: 3 },
    };
    applyFrame(model, state);
    expect(model.items).toHaveLength(2);
    expect(noticeFor(model)).toBe('Paused by the owner');
  });

  it('a new snapshot replaces the rows and the panes', () => {
    const model = emptyModel();
    applyFrame(model, { v: 1, kind: 'snap', panes: [p1], items: [user(1, 'p1', 'old')] });
    applyFrame(model, { v: 1, kind: 'snap', panes: [p2], items: [user(9, 'p2', 'new')] });
    expect(model.panes.map((pane) => pane.key)).toEqual(['p2']);
    expect(model.items).toEqual([user(9, 'p2', 'new')]);
  });

  it('skips a repeated item id in the same pane', () => {
    const model = emptyModel();
    applyFrame(model, { v: 1, kind: 'snap', items: [user(1, 'p1', 'once')] });
    applyFrame(model, { v: 1, kind: 'delta', items: [user(1, 'p1', 'once'), user(1, 'p2', 'other pane')] });
    expect(model.items).toHaveLength(2);
  });

  it('a plan with replace drops the earlier plan of that pane only', () => {
    const model = emptyModel();
    const plan = (id: number, pane: string, text: string): ItemView => ({
      t: 'plan',
      id,
      pane,
      tasks: [{ text, status: 'todo' }],
      replace: true,
    });
    applyFrame(model, { v: 1, kind: 'snap', items: [plan(1, 'p1', 'old'), plan(2, 'p2', 'other')] });
    applyFrame(model, { v: 1, kind: 'delta', items: [plan(3, 'p1', 'new')] });
    expect(model.items.map((item) => item.id)).toEqual([2, 3]);
  });

  it('keeps only the newest rows of each pane', () => {
    const model = emptyModel();
    const items: ItemView[] = [];
    for (let i = 1; i <= MAX_ITEMS_PER_PANE + 5; i++) {
      items.push(user(i, 'p1', `row ${i}`));
    }
    applyFrame(model, { v: 1, kind: 'snap', items: items.slice(0, 3) });
    applyFrame(model, { v: 1, kind: 'delta', items: items.slice(3) });
    expect(model.items).toHaveLength(MAX_ITEMS_PER_PANE);
    expect(model.items[0]?.id).toBe(6);
  });

  it('a delta can name a pane that is not known yet and merges by key', () => {
    const model = emptyModel();
    applyFrame(model, { v: 1, kind: 'delta', panes: [p1] });
    applyFrame(model, { v: 1, kind: 'delta', panes: [{ ...p1, title: 'Renamed' }] });
    expect(model.panes).toHaveLength(1);
    expect(model.panes[0]?.title).toBe('Renamed');
  });
});

describe('normalizeItem', () => {
  it('drops unknown kinds, rows without an id, and non-objects', () => {
    expect(normalizeItem({ t: 'secret', id: 1, pane: 'p1', text: 'x' })).toBeNull();
    expect(normalizeItem({ t: 'user', pane: 'p1', text: 'x' })).toBeNull();
    expect(normalizeItem('text')).toBeNull();
    expect(normalizeItem(null)).toBeNull();
  });

  it('keeps only the known fields of a tool row', () => {
    const item = normalizeItem({
      t: 'tool',
      id: 3,
      pane: 'p1',
      name: 'Bash',
      summary: 'npm test',
      files: 2,
      writes: true,
      input: { command: 'rm -rf x' },
    });
    expect(item).toEqual({ t: 'tool', id: 3, pane: 'p1', name: 'Bash', summary: 'npm test', files: 2, writes: true });
  });

  it('replaces a wrong type with an empty value instead of passing it on', () => {
    expect(normalizeItem({ t: 'user', id: 1, pane: 'p1', text: { html: '<b>x</b>' } })).toEqual({
      t: 'user',
      id: 1,
      pane: 'p1',
      text: '',
    });
  });
});

describe('noticeFor and endModel', () => {
  it.each([
    ['connecting', 'Connecting…'],
    ['reconnecting', 'Connection lost. Trying again…'],
    ['full', 'This Live Room is full'],
    ['ended', 'This Live Room has ended'],
    ['broken', 'This link does not work'],
  ] as const)('says the right sentence for %s', (status, text) => {
    expect(noticeFor({ ...emptyModel(), status })).toBe(text);
  });

  it('says some steps were not shared after a resume', () => {
    const model = emptyModel();
    applyFrame(model, { v: 1, kind: 'snap', room: { title: 't', endsAt: 1, paused: false, hidden: 4 } });
    expect(noticeFor(model)).toBe('Some steps were not shared');
  });

  it('says nothing when live and nothing is hidden', () => {
    const model = emptyModel();
    applyFrame(model, { v: 1, kind: 'snap', room: { title: 't', endsAt: 1, paused: false, hidden: 0 } });
    expect(noticeFor(model)).toBe('');
  });

  it('clears the rows and panes when the room ends', () => {
    const model = emptyModel();
    applyFrame(model, { v: 1, kind: 'snap', panes: [p1], items: [user(1, 'p1', 'hello')] });
    endModel(model);
    expect(model.status).toBe('ended');
    expect(model.items).toEqual([]);
    expect(model.panes).toEqual([]);
    expect(model.room).toBeNull();
  });
});

describe('labels', () => {
  it('reads durations and results like a person', () => {
    expect(durationLabel(null)).toBe('');
    expect(durationLabel(81_234)).toBe('1 min 21 s');
    expect(durationLabel(42_000)).toBe('42 s');
    expect(resultLabel({ t: 'result', id: 1, pane: 'p1', ok: false, lines: 37 })).toBe('failed · 37 lines');
    expect(resultLabel({ t: 'result', id: 1, pane: 'p1', ok: true, lines: 1 })).toBe('ok · 1 line');
  });

  it('builds the tool row side text', () => {
    expect(toolMeta({ t: 'tool', id: 1, pane: 'p1', name: 'Edit', summary: '', files: 1, writes: true })).toBe(
      '1 file · writes',
    );
    expect(toolMeta({ t: 'tool', id: 1, pane: 'p1', name: 'Bash', summary: '', files: 0, writes: false })).toBe('');
  });

  it('names every pane state', () => {
    expect(stateWord('working')).toBe('Working');
    expect(stateWord('waiting')).toBe('Waiting');
    expect(stateWord('error')).toBe('Error');
    expect(stateWord('gone')).toBe('Closed');
    expect(stateWord('idle')).toBe('Idle');
  });
});

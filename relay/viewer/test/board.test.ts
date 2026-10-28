// @vitest-environment jsdom
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { Board, type BoardElements } from '../src/board';
import { emptyModel, endModel, applyFrame, type Model } from '../src/render';
import { queryElements } from '../src/viewer';
import type { Frame } from '../src/types';

const html = readFileSync(join(__dirname, '..', 'src', 'viewer.html'), 'utf8');

/** A fresh copy of the real page markup. DOMParser builds it without running any script. */
function freshPage(): { doc: Document; els: BoardElements } {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  const els = queryElements(doc);
  if (!els) {
    throw new Error('page markup is missing a part');
  }
  return { doc, els };
}

function liveModel(frame: Frame): Model {
  const model = emptyModel();
  applyFrame(model, frame);
  return model;
}

const panes = [
  { key: 'p1', label: 'Agent A', title: 'Fix the login form', state: 'working' as const, line: 'Running npm test' },
  { key: 'p2', label: 'Agent B', title: 'Count lines', state: 'waiting' as const, line: 'Waiting for the owner' },
];

describe('Board', () => {
  it('draws the title, the panes and their chat rows as text', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    const model = liveModel({
      v: 1,
      kind: 'snap',
      room: { title: 'Bug fix stream', endsAt: 1, paused: false, hidden: 0 },
      panes,
      items: [
        { t: 'user', id: 1, pane: 'p1', text: 'Fix the login bug' },
        { t: 'agent', id: 2, pane: 'p1', text: "I'll look at auth.ts first." },
        { t: 'tool', id: 3, pane: 'p1', name: 'Bash', summary: 'npm test', files: 0, writes: false },
        { t: 'result', id: 4, pane: 'p1', ok: false, lines: 37 },
      ],
    });
    board.draw(model, { active: null, follow: true });

    expect(els.title.textContent).toBe('Bug fix stream');
    expect(els.notice.hidden).toBe(true);
    expect(els.live.hidden).toBe(false);
    const columns = doc.querySelectorAll('.lr-pane');
    expect(columns).toHaveLength(2);
    expect(columns[0]?.querySelector('.lr-pane-label')?.textContent).toBe('Agent A');
    expect(columns[0]?.querySelector('.lr-tool-name')?.textContent).toBe('Bash');
    expect(columns[0]?.querySelector('.lr-tool-summary')?.textContent).toBe('npm test');
    expect(columns[0]?.querySelector('.lr-result')?.textContent).toBe('failed · 37 lines');
    expect(columns[0]?.querySelectorAll('.lr-chat > li')).toHaveLength(4);
  });

  it('keeps markup in a chat text as plain text', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    const bad = '<img src=x onerror="alert(1)"><b>bold</b>';
    board.draw(
      liveModel({ v: 1, kind: 'snap', panes, items: [{ t: 'agent', id: 1, pane: 'p1', text: bad }] }),
      { active: null, follow: false },
    );
    expect(doc.querySelector('img')).toBeNull();
    expect(doc.querySelector('b')).toBeNull();
    expect(doc.querySelector('.lr-text')?.textContent).toBe(bad);
  });

  it('shows a waiting pane as a pill with the fixed line and no card text', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    board.draw(liveModel({ v: 1, kind: 'snap', panes }), { active: null, follow: false });
    const waiting = doc.querySelector('.lr-pane[data-key="p2"]');
    expect(waiting?.getAttribute('data-state')).toBe('waiting');
    expect(waiting?.querySelector('.lr-pill')?.textContent).toBe('Waiting for the owner');
    expect(doc.body.textContent).not.toContain('rm -rf');
  });

  it('marks the active tab and reports the active pane', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    const active = board.draw(liveModel({ v: 1, kind: 'snap', panes }), { active: 'p2', follow: false });
    expect(active).toBe('p2');
    expect(doc.querySelector('.lr-tab[data-key="p2"]')?.getAttribute('aria-pressed')).toBe('true');
    expect(doc.querySelector('.lr-tab[data-key="p1"]')?.getAttribute('aria-pressed')).toBe('false');
    expect(doc.querySelector('.lr-pane[data-key="p2"]')?.classList.contains('lr-pane--active')).toBe(true);
  });

  it('falls back to the first pane when the active one is gone', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    const active = board.draw(liveModel({ v: 1, kind: 'snap', panes }), { active: 'p9', follow: false });
    expect(active).toBe('p1');
  });

  it('calls onSelect with the pane key when a tab is pressed', () => {
    const { doc, els } = freshPage();
    const picked: string[] = [];
    const board = new Board(doc, els, { onSelect: (key) => picked.push(key), onFollow: () => {} });
    board.draw(liveModel({ v: 1, kind: 'snap', panes }), { active: 'p1', follow: false });
    doc.querySelector<HTMLButtonElement>('.lr-tab[data-key="p2"]')?.click();
    expect(picked).toEqual(['p2']);
  });

  it('toggles follow through the button and shows the state', () => {
    const { doc, els } = freshPage();
    const follows: boolean[] = [];
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: (on) => follows.push(on) });
    board.draw(liveModel({ v: 1, kind: 'snap', panes }), { active: null, follow: true });
    expect(els.follow.getAttribute('aria-pressed')).toBe('true');
    els.follow.click();
    expect(follows).toEqual([false]);
  });

  it('shows the sentence for a broken link and no chat', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    board.draw({ ...emptyModel(), status: 'broken' }, { active: null, follow: true });
    expect(els.notice.textContent).toBe('This link does not work');
    expect(els.notice.hidden).toBe(false);
    expect(doc.querySelectorAll('.lr-pane')).toHaveLength(0);
  });

  it('clears the chat when the room ends', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    const model = liveModel({ v: 1, kind: 'snap', panes, items: [{ t: 'note', id: 1, pane: 'p1', text: 'hi' }] });
    board.draw(model, { active: null, follow: true });
    endModel(model);
    board.draw(model, { active: null, follow: true });
    expect(doc.querySelectorAll('.lr-chat > li')).toHaveLength(0);
    expect(els.notice.textContent).toBe('This Live Room has ended');
    expect(els.live.hidden).toBe(true);
  });

  it('shows the paused sentence', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    board.draw(
      liveModel({ v: 1, kind: 'snap', room: { title: 't', endsAt: 1, paused: true, hidden: 0 }, panes }),
      { active: null, follow: false },
    );
    expect(els.notice.textContent).toBe('Paused by the owner');
  });

  it('the page has no input, textarea, form or editable element', () => {
    const { doc, els } = freshPage();
    const board = new Board(doc, els, { onSelect: () => {}, onFollow: () => {} });
    board.draw(
      liveModel({ v: 1, kind: 'snap', panes, items: [{ t: 'user', id: 1, pane: 'p1', text: 'x' }] }),
      { active: null, follow: true },
    );
    expect(doc.querySelector('input, textarea, form, select, [contenteditable]')).toBeNull();
  });
});

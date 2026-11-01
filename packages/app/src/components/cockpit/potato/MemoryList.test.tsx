// @vitest-environment jsdom
// Memory of a bot: the notes newest first, the text of one note, Delete at once with an Undo line, the empty line, and
// the sentences for a failed read or delete.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeMemoryTransport, type MemoryTransport } from '../../../lib/potato-memory';
import { fakeTrashTransport, type FakeTrashTransport } from '../../../lib/potato-trash';
import { MEMORY_COPY, MemoryList, NOTES_SHOWN_AT_FIRST, noteSize } from './MemoryList';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const NOTES = [
  { path: 'nodes/people/sam.md', body: '# Sam\nLikes short emails.', modifiedMs: 1_700_000_300_000 },
  { path: 'journal/2026/10/2026-10-28.md', body: '# day\n- 10:00 went for a walk', modifiedMs: 1_700_000_200_000 },
];

async function show(transport: MemoryTransport, trash: FakeTrashTransport = fakeTrashTransport()) {
  await act(async () => root.render(<MemoryList botId="nimbus" transport={transport} trash={trash} />));
  await act(async () => {});
}

const rowOpeners = () => [...host.querySelectorAll<HTMLButtonElement>('.ck-potato-note-open')];
const titles = () => [...host.querySelectorAll('.ck-potato-note-title')].map((t) => t.textContent);
const buttonNamed = (label: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim() === label);
const alertText = () => host.querySelector('[role="alert"]')?.textContent;

describe('Memory', () => {
  it('lists the notes with their titles, folder and size', async () => {
    await show(fakeMemoryTransport({ notes: NOTES }));
    expect(titles()).toEqual(['sam', '2026-10-28']);
    expect(rowOpeners()[0].textContent).toContain('Note');
    expect(rowOpeners()[1].textContent).toContain('Journal');
    expect(rowOpeners()[0].textContent).toContain(noteSize(NOTES[0].body.length));
  });

  it('says so when nothing is remembered yet', async () => {
    await show(fakeMemoryTransport());
    expect(host.textContent).toContain(MEMORY_COPY.empty);
  });

  it('opens one note to its text, and closes it again', async () => {
    const transport = fakeMemoryTransport({ notes: NOTES });
    await show(transport);
    await act(async () => rowOpeners()[0].click());
    expect(host.querySelector('.ck-potato-note-text')?.textContent).toBe('# Sam\nLikes short emails.');
    expect(transport.calls.at(-1)).toEqual({ method: 'read', args: ['nimbus', 'nodes/people/sam.md'] });
    await act(async () => rowOpeners()[0].click());
    expect(host.querySelector('.ck-potato-note-text')).toBeNull();
  });

  it('Delete removes the note at once and offers Undo', async () => {
    const transport = fakeMemoryTransport({ notes: NOTES });
    await show(transport);
    await act(async () => rowOpeners()[0].click());
    await act(async () => buttonNamed(MEMORY_COPY.delete)!.click());
    expect(transport.calls.find((c) => c.method === 'remove')?.args).toEqual(['nimbus', 'nodes/people/sam.md']);
    expect(titles()).toEqual(['2026-10-28']);
    expect(host.querySelector('.ck-potato-note-text')).toBeNull();
    expect(host.textContent).toContain(MEMORY_COPY.deleted);
    expect(buttonNamed('Undo')).toBeDefined();
  });

  it('Undo brings the note back from Recently deleted', async () => {
    const trash = fakeTrashTransport();
    const transport = fakeMemoryTransport({ notes: NOTES });
    await show(transport, trash);
    await act(async () => rowOpeners()[0].click());
    await act(async () => buttonNamed(MEMORY_COPY.delete)!.click());
    await act(async () => buttonNamed('Undo')!.click());
    expect(trash.calls).toEqual([{ method: 'restore', args: ['trash-nodes/people/sam.md'] }]);
    expect(host.textContent).not.toContain(MEMORY_COPY.deleted);
  });

  it('shows a sentence when a note cannot be opened', async () => {
    const base = fakeMemoryTransport({ notes: NOTES });
    await show({ ...base, read: () => Promise.reject(new Error(MEMORY_COPY.readFailed)) });
    await act(async () => rowOpeners()[0].click());
    expect(alertText()).toBe(MEMORY_COPY.readFailed);
  });

  it('shows a sentence when a delete fails and keeps the note', async () => {
    const base = fakeMemoryTransport({ notes: NOTES });
    await show({ ...base, remove: () => Promise.reject(new Error(MEMORY_COPY.deleteFailed)) });
    await act(async () => rowOpeners()[0].click());
    await act(async () => buttonNamed(MEMORY_COPY.delete)!.click());
    expect(alertText()).toBe(MEMORY_COPY.deleteFailed);
    expect(titles()).toHaveLength(2);
  });

  it('shows the first notes and opens the rest with Show all', async () => {
    const many = Array.from({ length: NOTES_SHOWN_AT_FIRST + 2 }, (_, i) => ({ path: `nodes/n${i}.md`, body: 'x', modifiedMs: i }));
    await show(fakeMemoryTransport({ notes: many }));
    expect(rowOpeners()).toHaveLength(NOTES_SHOWN_AT_FIRST);
    await act(async () => buttonNamed(MEMORY_COPY.showMore(NOTES_SHOWN_AT_FIRST + 2))!.click());
    expect(rowOpeners()).toHaveLength(NOTES_SHOWN_AT_FIRST + 2);
  });

  it('shows a sentence when the list cannot be read', async () => {
    await show(fakeMemoryTransport({ fail: 'Could not reach the notes.' }));
    expect(alertText()).toBe('Could not reach the notes.');
  });
});

describe('noteSize', () => {
  it('reads bytes below a kilobyte and rounds the rest to kilobytes', () => {
    expect(noteSize(120)).toBe('120 B');
    expect(noteSize(2048)).toBe('2 KB');
  });
});

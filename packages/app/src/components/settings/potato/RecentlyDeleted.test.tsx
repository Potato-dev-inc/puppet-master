// @vitest-environment jsdom
// Settings > Potato Bot > Recently deleted: the list of what was deleted in the last 7 days, the days left, Bring back
// (it calls restore and the row goes away), the empty line, and a sentence when a restore fails.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { fakeTrashTransport, type TrashItem } from '../../../lib/potato-trash';
import { RecentlyDeletedList, TRASH_COPY } from './RecentlyDeleted';

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

const ITEMS: TrashItem[] = [
  { id: 'routine-1', kind: 'routine', bot: 'nimbus', title: 'Morning sweep', deletedAt: Date.now() - 60_000 },
  { id: 'bot-2', kind: 'bot', bot: 'fish', title: 'Fish', deletedAt: Date.now() - 2 * 24 * 60 * 60 * 1000 },
];

const flush = () =>
  act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });

const rows = () => [...host.querySelectorAll<HTMLElement>('.ck-potato-trash-row')];
const bringBack = (title: string) => [...host.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.getAttribute('aria-label') === `${TRASH_COPY.bring} ${title}`);

describe('Recently deleted', () => {
  it('lists what was deleted, with its kind and the days left', async () => {
    await act(async () => root.render(<RecentlyDeletedList transport={fakeTrashTransport(ITEMS)} />));
    await flush();
    expect(rows().map((r) => r.querySelector('.ck-potato-trash-name')?.textContent)).toEqual(['Morning sweep', 'Fish']);
    expect(rows()[0].textContent).toContain('Routine');
    expect(rows()[1].textContent).toContain('Bot');
    expect(rows()[1].textContent).toContain('5 days left');
  });

  it('Bring back restores the item and its row goes away', async () => {
    const transport = fakeTrashTransport(ITEMS);
    await act(async () => root.render(<RecentlyDeletedList transport={transport} />));
    await flush();
    await act(async () => bringBack('Fish')!.click());
    await flush();
    expect(transport.calls.find((c) => c.method === 'restore')?.args).toEqual(['bot-2']);
    expect(rows().map((r) => r.querySelector('.ck-potato-trash-name')?.textContent)).toEqual(['Morning sweep']);
  });

  it('says so when nothing was deleted lately', async () => {
    await act(async () => root.render(<RecentlyDeletedList transport={fakeTrashTransport([])} />));
    await flush();
    expect(host.textContent).toContain(TRASH_COPY.empty);
  });

  it('shows a sentence when a restore fails and keeps the row', async () => {
    const base = fakeTrashTransport(ITEMS);
    await act(async () =>
      root.render(<RecentlyDeletedList transport={{ ...base, restore: () => Promise.reject(new Error(TRASH_COPY.restoreFailed)) }} />),
    );
    await flush();
    await act(async () => bringBack('Fish')!.click());
    await flush();
    expect(host.querySelector('[role="alert"]')?.textContent).toBe(TRASH_COPY.restoreFailed);
    expect(rows()).toHaveLength(2);
  });
});

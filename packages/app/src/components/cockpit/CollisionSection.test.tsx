// @vitest-environment jsdom
// The collision cards in the Inbox: loud cards with their buttons, the quiet "No clash yet" group,
// Ignore, the event that re-reads the list, and the footer that ends every card.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { CollisionApi, CollisionView } from '../../lib/collision';
import { COLLISION_FOOTER, CollisionSection } from './CollisionSection';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ID = 'ca-0123456789ab';
const QUIET_ID = 'ca-ba9876543210';

function view(over: Partial<CollisionView> = {}): CollisionView {
  return {
    id: ID,
    level: 'clash',
    relation: 'copies',
    title: 'fake_a and fake_b both changed src/auth.ts',
    detail: 'Lines 40-80 overlap.',
    advice: 'Finish fake_a first. It started first. Merge it first. Then fake_b can build on it.',
    place: 'In separate copies.',
    footer: COLLISION_FOOTER,
    sides: [
      { paneId: 'pane-a', label: 'fake_a', working: true },
      { paneId: 'pane-b', label: 'fake_b', working: true },
    ],
    files: [{ path: 'src/auth.ts', text: 'Lines 40-80' }],
    more: 0,
    firstMs: 1000,
    lastMs: 2000,
    ...over,
  };
}

const quiet = (over: Partial<CollisionView> = {}): CollisionView =>
  view({
    id: QUIET_ID,
    level: 'same_file',
    relation: 'copies',
    title: 'fake_a and fake_b both changed src/db.ts',
    detail: 'Different places in the file. No clash yet.',
    advice: null,
    place: 'In separate copies.',
    files: [{ path: 'src/db.ts', text: 'Different places' }],
    ...over,
  });

function fakeApi(rows: CollisionView[] = []) {
  let changed: (() => void) | undefined;
  const unlisten = vi.fn();
  const api: CollisionApi = {
    list: vi.fn(() => Promise.resolve(rows)),
    ignore: vi.fn(() => Promise.resolve()),
    settingsGet: vi.fn(() => Promise.resolve({ enabled: true })),
    settingsSet: vi.fn((s) => Promise.resolve(s)),
    onChanged: vi.fn((cb: () => void) => {
      changed = cb;
      return Promise.resolve(unlisten);
    }),
  };
  return { api, unlisten, fire: () => changed?.() };
}

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
  vi.clearAllMocks();
});

/** Mount and let the first list read settle. */
async function mount(api: CollisionApi, hasPane: (id: string) => boolean = () => true, select = vi.fn()) {
  await act(async () => {
    root.render(<CollisionSection select={select} hasPane={hasPane} api={api} />);
  });
  return select;
}

const buttons = (el: Element) => Array.from(el.querySelectorAll('button')).map((b) => b.textContent);
const card = (id: string) => host.querySelector<HTMLElement>(`li[data-collision-id="${id}"]`);

describe('CollisionSection', () => {
  it('renders nothing when there are no alerts', async () => {
    const { api } = fakeApi([]);
    await mount(api);
    expect(host.querySelector('section')).toBeNull();
    expect(host.textContent).toBe('');
  });

  it('shows a clash card with title, place, detail, advice, buttons and the footer', async () => {
    const { api } = fakeApi([view()]);
    await mount(api);
    const li = card(ID)!;
    expect(li.className).toContain('ck-inbox-item');
    expect(li.className).toContain('ck-collision-card');
    expect(li.className).not.toContain('ck-collision-quiet');
    expect(li.querySelector('.ck-collision-title')?.textContent).toBe('fake_a and fake_b both changed src/auth.ts');
    expect(li.querySelector('.ck-collision-place')?.textContent).toBe('In separate copies.');
    expect(li.querySelector('.ck-collision-detail')?.textContent).toBe('Lines 40-80 overlap.');
    expect(li.querySelector('.ck-collision-advice')?.textContent).toContain('Finish fake_a first.');
    expect(buttons(li)).toEqual(['Open fake_a', 'Open fake_b', 'Ignore']);
    expect(li.querySelector('.ck-collision-footer')?.textContent).toBe('Nothing was paused. You decide.');
  });

  it('ends every card with the footer as its last line', async () => {
    const { api } = fakeApi([view(), view({ id: 'ca-111111111111', level: 'shared_folder', relation: 'same_folder', advice: null }), quiet()]);
    await mount(api);
    const cards = Array.from(host.querySelectorAll<HTMLElement>('li.ck-collision-card'));
    expect(cards).toHaveLength(3);
    for (const li of cards) {
      expect(li.lastElementChild?.textContent).toBe('Nothing was paused. You decide.');
    }
  });

  it('opens a pane with select(paneId) from its Open button', async () => {
    const { api } = fakeApi([view()]);
    const select = await mount(api);
    const open = Array.from(card(ID)!.querySelectorAll('button')).find((b) => b.textContent === 'Open fake_b')!;
    act(() => open.click());
    expect(select).toHaveBeenCalledWith('pane-b');
  });

  it('hides the Open button of a pane that is gone', async () => {
    const { api } = fakeApi([view()]);
    await mount(api, (id) => id !== 'pane-b');
    expect(buttons(card(ID)!)).toEqual(['Open fake_a', 'Ignore']);
  });

  it('Ignore calls collisionIgnore with the id and the card leaves', async () => {
    const { api } = fakeApi([view(), quiet()]);
    await mount(api);
    const ignore = Array.from(card(ID)!.querySelectorAll('button')).find((b) => b.textContent === 'Ignore')!;
    await act(async () => {
      ignore.click();
    });
    expect(api.ignore).toHaveBeenCalledWith(ID);
    expect(card(ID)).toBeNull();
    expect(card(QUIET_ID)).not.toBeNull();
  });

  it('brings the card back by reading the list again when Ignore fails', async () => {
    const { api } = fakeApi([view()]);
    vi.mocked(api.ignore).mockRejectedValueOnce(new Error('COLLISION_NOT_FOUND: no such alert'));
    await mount(api);
    const ignore = Array.from(card(ID)!.querySelectorAll('button')).find((b) => b.textContent === 'Ignore')!;
    await act(async () => {
      ignore.click();
    });
    await act(async () => {});
    expect(api.list).toHaveBeenCalledTimes(2);
    expect(card(ID)).not.toBeNull();
  });

  it('puts a same_file item under "No clash yet" with only the title, detail and Ignore', async () => {
    const { api } = fakeApi([view(), quiet()]);
    await mount(api);
    const heading = Array.from(host.querySelectorAll('[role="heading"]')).find((h) => h.textContent === 'No clash yet');
    expect(heading).toBeDefined();
    const li = card(QUIET_ID)!;
    expect(li.className).toContain('ck-collision-quiet');
    expect(li.querySelector('.ck-collision-advice')).toBeNull();
    expect(li.querySelector('.ck-collision-place')).toBeNull();
    expect(li.querySelector('.ck-collision-title')?.textContent).toBe('fake_a and fake_b both changed src/db.ts');
    expect(li.querySelector('.ck-collision-detail')?.textContent).toBe('Different places in the file. No clash yet.');
    expect(buttons(li)).toEqual(['Ignore']);
    // The heading comes after the loud cards.
    const loud = card(ID)!;
    expect(loud.compareDocumentPosition(heading!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('lists up to five files and says how many more there are', async () => {
    const files = ['a.ts', 'b.ts', 'c.ts', 'd.ts', 'e.ts'].map((p) => ({ path: `src/${p}`, text: 'Line 4' }));
    const { api } = fakeApi([view({ files, more: 2, title: 'fake_a and fake_b both changed src/a.ts and 6 more files' })]);
    await mount(api);
    const items = Array.from(card(ID)!.querySelectorAll('.ck-collision-files li')).map((li) => li.textContent);
    expect(items).toEqual([...files.map((f) => `${f.path} - Line 4`), 'and 2 more']);
  });

  it('shows no file list for a single file', async () => {
    const { api } = fakeApi([view()]);
    await mount(api);
    expect(card(ID)!.querySelector('.ck-collision-files')).toBeNull();
  });

  it('shows file names as plain text, never as markup', async () => {
    const weird = 'src/<img src=x>.ts';
    const { api } = fakeApi([view({ files: [{ path: weird, text: 'Lines 1-2' }, { path: 'src/b.ts', text: 'Line 9' }] })]);
    await mount(api);
    const li = card(ID)!;
    expect(li.querySelector('img')).toBeNull();
    expect(li.textContent).toContain(`${weird} - Lines 1-2`);
  });

  it('reads the list again when Rust says it changed, and stops listening on unmount', async () => {
    const { api, fire, unlisten } = fakeApi([view()]);
    await mount(api);
    expect(api.list).toHaveBeenCalledTimes(1);
    vi.mocked(api.list).mockResolvedValueOnce([quiet()]);
    await act(async () => {
      fire();
    });
    expect(api.list).toHaveBeenCalledTimes(2);
    expect(card(ID)).toBeNull();
    expect(card(QUIET_ID)).not.toBeNull();
    act(() => root.unmount());
    expect(unlisten).toHaveBeenCalledTimes(1);
    root = createRoot(host);
  });

  it('shows nothing and does not throw when the list cannot be read', async () => {
    const { api } = fakeApi();
    vi.mocked(api.list).mockRejectedValueOnce(new Error('Desktop shell required: collision_list'));
    await mount(api);
    expect(host.querySelector('section')).toBeNull();
  });
});

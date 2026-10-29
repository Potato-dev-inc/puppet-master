// @vitest-environment jsdom
// The Collision Alert row above a chat: it shows for the two panes of a clash or a shared folder, not for a third
// pane or a quiet same-file alert. It reads the list through lib/collision.ts (invoke is mocked), Ignore hides it at
// once, and it reloads when `collision://changed` fires.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const invoke = vi.fn();
const handlers = new Map<string, (e: { payload: unknown }) => void>();
const listen = vi.fn(async (event: string, cb: (e: { payload: unknown }) => void) => {
  handlers.set(event, cb);
  return () => handlers.delete(event);
});
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invoke(...a) }));
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listen(...(a as Parameters<typeof listen>)) }));

import type { CollisionView } from '../../lib/collision';
import { bannerText, CollisionBanner, pickBanner } from './CollisionBanner';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const ID = 'ca-0123456789ab';

// Fake labels only.
function view(over: Partial<CollisionView> = {}): CollisionView {
  return {
    id: ID,
    level: 'clash',
    relation: 'copies',
    title: 'fake_a and fake_b both changed src/auth.ts',
    detail: 'Lines 40-80 overlap.',
    advice: 'Finish fake_a first. It started first.',
    place: 'In separate copies.',
    footer: 'Nothing was paused. You decide.',
    sides: [
      { paneId: 'pane-a', label: 'fake_a', working: true },
      { paneId: 'pane-b', label: 'fake_b', working: false },
    ],
    files: [{ path: 'src/auth.ts', text: 'Lines 40-80' }],
    more: 0,
    firstMs: 1000,
    lastMs: 2000,
    ...over,
  };
}

let host: HTMLDivElement;
let root: Root;

const flush = () => act(async () => { await new Promise((r) => setTimeout(r, 0)); });
const row = () => host.querySelector<HTMLElement>('[data-collision-banner]');
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);

/** Mounts the banner for `paneId` with the alert list the mocked shell returns. */
async function mount(paneId: string, list: CollisionView[] | Error, onOpenPane?: (paneId: string) => void) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === 'collision_list') {
      if (list instanceof Error) throw list;
      return list;
    }
    return undefined;
  });
  await act(async () => {
    root.render(<CollisionBanner paneId={paneId} onOpenPane={onOpenPane} />);
  });
  await flush();
}

beforeEach(() => {
  (window as unknown as { __TAURI_INTERNALS__: object }).__TAURI_INTERNALS__ = {};
  invoke.mockReset();
  listen.mockClear();
  handlers.clear();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
});

describe('CollisionBanner: who sees it', () => {
  it('shows one row to the first pane of a clash and names the other pane', async () => {
    await mount('pane-a', [view()]);
    expect(row()).not.toBeNull();
    expect(row()?.dataset.collisionBanner).toBe('clash');
    expect(row()?.textContent).toContain('fake_b also changed src/auth.ts');
    expect(row()?.textContent).toContain('Lines 40-80');
  });

  it('shows the row to the second pane too, naming the first one', async () => {
    await mount('pane-b', [view()]);
    expect(row()?.textContent).toContain('fake_a also changed src/auth.ts');
  });

  it('shows nothing to a third pane', async () => {
    await mount('pane-c', [view()]);
    expect(row()).toBeNull();
  });

  it('shows nothing for a quiet same-file alert', async () => {
    await mount('pane-a', [view({ level: 'same_file' })]);
    expect(row()).toBeNull();
  });

  it('shows nothing when there is no alert at all', async () => {
    await mount('pane-a', []);
    expect(row()).toBeNull();
  });

  it('shows nothing when the list cannot be read (no desktop shell)', async () => {
    await mount('pane-a', new Error('Desktop shell required: collision_list'));
    expect(row()).toBeNull();
  });
});

describe('CollisionBanner: the words', () => {
  it('uses the file text for a line overlap', async () => {
    await mount('pane-a', [view({ files: [{ path: 'src/auth.ts', text: 'Lines 40-80' }] })]);
    expect(row()?.textContent).toContain('Lines 40-80');
  });

  it('shows the short text for both-new files', async () => {
    await mount('pane-a', [view({ files: [{ path: 'notes.md', text: 'Both made a new file.' }] })]);
    expect(row()?.textContent).toContain('fake_b also changed notes.md');
    expect(row()?.textContent).toContain('Both made a new file.');
  });

  it('says the same-folder words for a shared folder, without a file claim', async () => {
    await mount('pane-a', [view({ level: 'shared_folder', files: [] })]);
    expect(row()?.dataset.collisionBanner).toBe('shared_folder');
    expect(row()?.textContent).toContain('fake_b is working in the same folder');
    expect(row()?.textContent).not.toContain('also changed');
  });

  it('counts the files it does not list', async () => {
    await mount('pane-a', [view({ files: [{ path: 'a.ts', text: 'Line 1 overlaps.' }, { path: 'b.ts', text: '' }], more: 2 })]);
    expect(row()?.textContent).toContain('and 3 more files');
  });

  it('keeps the words pure: bannerText and pickBanner', () => {
    const v = view();
    expect(bannerText(v, v.sides[1])).toEqual({ lead: 'fake_b also changed src/auth.ts', note: 'Lines 40-80' });
    expect(bannerText(view({ files: [{ path: 'x.ts', text: '' }], more: 0 }), v.sides[1]).note).toBeNull();
    expect(bannerText(view({ files: [], more: 0 }), v.sides[1])).toEqual({ lead: v.title, note: null });
    expect(bannerText(view({ files: [{ path: 'a.ts', text: '' }, { path: 'b.ts', text: '' }], more: 0 }), v.sides[1]).lead).toBe(
      'fake_b also changed a.ts and 1 more file',
    );
    expect(pickBanner([view({ level: 'same_file' }), view({ id: 'ca-aaaaaaaaaaaa' })], 'pane-a')?.view.id).toBe('ca-aaaaaaaaaaaa');
    expect(pickBanner([view()], 'pane-z')).toBeNull();
  });
});

describe('CollisionBanner: actions', () => {
  it('Open calls the pane-focus callback with the other pane', async () => {
    const onOpenPane = vi.fn();
    await mount('pane-a', [view()], onOpenPane);
    button('Open fake_b')?.click();
    expect(onOpenPane).toHaveBeenCalledWith('pane-b');
  });

  it('has no Open button without a callback', async () => {
    await mount('pane-a', [view()]);
    expect(button('Open fake_b')).toBeUndefined();
    expect(button('Ignore')).toBeDefined();
  });

  it('Ignore calls collision_ignore and the row leaves at once', async () => {
    await mount('pane-a', [view()]);
    await act(async () => {
      button('Ignore')?.click();
    });
    expect(invoke).toHaveBeenCalledWith('collision_ignore', { id: ID });
    expect(row()).toBeNull();
  });

  it('brings the row back when the shell refuses Ignore', async () => {
    await mount('pane-a', [view()]);
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === 'collision_ignore') throw new Error('COLLISION_NOT_FOUND: no such alert');
      if (cmd === 'collision_list') return [view()];
      return undefined;
    });
    await act(async () => {
      button('Ignore')?.click();
    });
    await flush();
    expect(row()).not.toBeNull();
  });
});

describe('CollisionBanner: refresh', () => {
  it('reads the list again when collision://changed fires', async () => {
    await mount('pane-a', []);
    expect(row()).toBeNull();
    invoke.mockImplementation(async (cmd: string) => (cmd === 'collision_list' ? [view()] : undefined));
    await act(async () => {
      handlers.get('collision://changed')?.({ payload: {} });
    });
    await flush();
    expect(row()?.textContent).toContain('fake_b also changed src/auth.ts');
  });

  it('stops listening when it unmounts', async () => {
    await mount('pane-a', []);
    expect(handlers.has('collision://changed')).toBe(true);
    act(() => root.unmount());
    expect(handlers.has('collision://changed')).toBe(false);
    root = createRoot(host);
  });
});

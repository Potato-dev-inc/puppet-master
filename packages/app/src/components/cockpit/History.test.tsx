// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mocked } from 'vitest';
import type { Checkpoint, CheckpointList, CheckpointsApi } from '../../lib/cockpit-api';
import { History } from './History';
import {
  checkpointTitle,
  folderKey,
  formatAgo,
  formatDelta,
  panesSharingFolder,
  sharedFolderWarning,
} from './history-model';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const NOW = 1_800_000_000_000;

function cp(id: string, label: string, ago: number, extra: Partial<Checkpoint> = {}): Checkpoint {
  return {
    id,
    commit: `c${id}`,
    paneId: 'p1',
    turn: 1,
    kind: 'before',
    label,
    note: '',
    atMs: NOW - ago,
    files: 2,
    added: 10,
    deleted: 3,
    ...extra,
  };
}

const LISTING: CheckpointList = {
  state: 'ok',
  message: '',
  items: [
    cp('3', 'After turn 2', 60_000, { kind: 'after', files: 1, added: 4, deleted: 0 }),
    cp('2', 'Before turn 2', 10 * 60_000, { note: 'Add a login page' }),
    cp('1', 'Before turn 1', 3_600_000 * 3, { files: 0, added: 0, deleted: 0 }),
  ],
};

function mockApi(listing: CheckpointList = LISTING): Mocked<CheckpointsApi> {
  return {
    checkpointsList: vi.fn(async () => listing),
    checkpointDiff: vi.fn(async () => [
      { path: 'src/login.tsx', status: 'A', added: 40, deleted: 0, binary: false },
      { path: 'logo.png', status: 'M', added: 0, deleted: 0, binary: true },
    ]),
    checkpointRestore: vi.fn(async () => ({ safetyId: '4', written: 2, removed: 1, failed: [] })),
    checkpointUndoLast: vi.fn(async () => ({ safetyId: '4', written: 2, removed: 1, failed: [] })),
  } as unknown as Mocked<CheckpointsApi>;
}

let host: HTMLDivElement;
let root: Root;

async function mount(api: CheckpointsApi, status = 'idle', notify = vi.fn()) {
  await act(async () => {
    root.render(<History paneId="p1" status={status} now={NOW} notify={notify} api={api} />);
  });
  return notify;
}

const click = async (el: Element | null | undefined) => {
  expect(el).toBeTruthy();
  await act(async () => {
    (el as HTMLElement).click();
  });
};
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('History timeline', () => {
  it('lists checkpoints newest first with time, label, note and a +/- summary', async () => {
    await mount(mockApi());
    const rows = Array.from(host.querySelectorAll('.ck-hist-item'));
    expect(rows).toHaveLength(3);
    expect(rows[0].textContent).toContain('After turn 2');
    expect(rows[0].textContent).toContain('1 min ago');
    expect(rows[0].textContent).toContain('1 file +4 −0');
    expect(rows[1].textContent).toContain('Add a login page');
    expect(rows[1].textContent).toContain('10 min ago');
    expect(rows[2].textContent).toContain('3 h ago');
    expect(rows[2].textContent).toContain('No file changes');
    expect(host.textContent).toContain("doesn't change what the agent remembers");
  });

  it('expands a row to the files that changed', async () => {
    const api = mockApi();
    await mount(api);
    await click(host.querySelectorAll('.ck-hist-main')[1]);
    expect(api.checkpointDiff).toHaveBeenCalledWith('p1', '2');
    expect(host.textContent).toContain('src/login.tsx');
    expect(host.textContent).toContain('+40 −0');
    expect(host.textContent).toContain('binary');
  });

  it('shows plain empty, off and no-git states', async () => {
    await mount(mockApi({ state: 'ok', message: '', items: [] }));
    expect(host.textContent).toContain('No checkpoints yet');
    await mount(mockApi({ state: 'off', message: 'off', items: [] }));
    expect(host.textContent).toContain('Checkpoints are off');
    await mount(mockApi({ state: 'no_git', message: "Checkpoints need Git, and Git isn't installed on this computer.", items: [] }));
    expect(host.textContent).toContain("Git isn't installed");
    await mount(mockApi({ state: 'too_big', message: 'Undo is off for this folder: it is too big. Add big folders to .gitignore.', items: [] }));
    expect(host.textContent).toContain('Undo is off for this folder');
    expect(host.textContent).toContain('Add big folders to .gitignore.');
    expect(host.querySelector('.ck-hist')).toBeNull();
  });

  it('shows the error when the list cannot be read', async () => {
    const api = mockApi();
    api.checkpointsList.mockRejectedValueOnce(new Error('boom'));
    await mount(api);
    expect(host.textContent).toContain('History unavailable');
    expect(host.textContent).toContain('boom');
  });
});

describe('Restore confirm sheet', () => {
  it('asks first, says files-only, and restores only after the confirm', async () => {
    const api = mockApi();
    const notify = await mount(api);
    await click(host.querySelectorAll('.ck-hist-row .ck-btn')[1]);
    const sheet = host.querySelector('[role="alertdialog"]');
    expect(sheet).toBeTruthy();
    expect(sheet!.textContent).toContain('Restore "Before turn 2"?');
    expect(sheet!.textContent).toContain("This restores your files. It doesn't change what the agent remembers.");
    expect(sheet!.textContent).toContain('saved first');
    expect(api.checkpointRestore).not.toHaveBeenCalled();

    await click(button('Restore files'));
    expect(api.checkpointRestore).toHaveBeenCalledWith('p1', '2');
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(notify).toHaveBeenCalledWith(
      'Files restored to "Before turn 2".',
      expect.objectContaining({ label: 'Undo' }),
    );
    expect(api.checkpointsList.mock.calls.length).toBeGreaterThanOrEqual(2);
  });

  it('the restore toast has an Undo that restores the "Before undo" checkpoint', async () => {
    const api = mockApi();
    const notify = await mount(api);
    await click(host.querySelectorAll('.ck-hist-row .ck-btn')[1]);
    await click(button('Restore files'));
    const action = notify.mock.calls[0][1] as { label: string; run: () => void };
    expect(api.checkpointRestore).toHaveBeenCalledTimes(1);
    await act(async () => action.run());
    // mockApi's restore reports safetyId "4": that is the checkpoint the Undo goes back to.
    expect(api.checkpointRestore).toHaveBeenLastCalledWith('p1', '4');
    expect(notify).toHaveBeenLastCalledWith('Undone. Your files are back as they were before the restore.');
  });

  it('offers no Undo when the restore saved nothing first', async () => {
    const api = mockApi();
    api.checkpointRestore.mockResolvedValueOnce({ safetyId: null, written: 0, removed: 0, failed: [] });
    const notify = await mount(api);
    await click(host.querySelectorAll('.ck-hist-row .ck-btn')[1]);
    await click(button('Restore files'));
    expect(notify).toHaveBeenCalledWith('Files restored to "Before turn 2".', undefined);
  });

  it('Cancel closes the sheet and restores nothing', async () => {
    const api = mockApi();
    await mount(api);
    await click(host.querySelectorAll('.ck-hist-row .ck-btn')[0]);
    await click(button('Cancel'));
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(api.checkpointRestore).not.toHaveBeenCalled();
  });

  it('disables Restore while the agent is running', async () => {
    const api = mockApi();
    await mount(api, 'running');
    const restore = Array.from(host.querySelectorAll<HTMLButtonElement>('.ck-hist-row .ck-btn'));
    expect(restore.every((b) => b.disabled)).toBe(true);
    expect(restore[0].title).toBe('Interrupt the agent first');
    expect(host.textContent).toContain('interrupt it before restoring');
  });

  it('reports a refused restore without crashing', async () => {
    const api = mockApi();
    api.checkpointRestore.mockRejectedValueOnce(new Error('The agent is working. Interrupt it first, then undo.'));
    const notify = await mount(api);
    await click(host.querySelectorAll('.ck-hist-row .ck-btn')[0]);
    await click(button('Restore files'));
    expect(notify).toHaveBeenCalledWith('The agent is working. Interrupt it first, then undo.');
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
  });
});

describe('history-model', () => {
  it('formats ages, titles and deltas', () => {
    expect(formatAgo(5_000)).toBe('just now');
    expect(formatAgo(5 * 60_000)).toBe('5 min ago');
    expect(formatAgo(2 * 3_600_000)).toBe('2 h ago');
    expect(formatAgo(3 * 86_400_000)).toBe('3 d ago');
    expect(formatAgo(-10)).toBe('just now');
    expect(checkpointTitle({ label: '' })).toBe('Checkpoint');
    expect(formatDelta({ files: 1, added: 2, deleted: 3 })).toBe('1 file +2 −3');
    expect(formatDelta({ files: 0, added: 0, deleted: 0 })).toBe('No file changes');
  });
});

describe('Restore warns when another pane works in the same folder', () => {
  const open = async (sharedWith: string[]) => {
    const api = mockApi();
    await act(async () => {
      root.render(<History paneId="p1" status="idle" now={NOW} notify={vi.fn()} api={api} sharedWith={sharedWith} />);
    });
    await click(host.querySelectorAll('.ck-hist-row .ck-btn')[0]);
    return host.querySelector('[role="alertdialog"]');
  };

  it('shows the line naming the other agent when a pane shares the folder', async () => {
    const sheet = await open(['Codex']);
    expect(sheet!.textContent).toContain('Codex also works in this folder. Its changes since then will be undone too.');
  });

  it('names up to two agents, then "and N more"', async () => {
    expect((await open(['Codex', 'Claude']))!.textContent).toContain(
      'Codex and Claude also work in this folder. Their changes since then will be undone too.',
    );
    expect((await open(['Codex', 'Claude', 'Kilo', 'Gemini']))!.textContent).toContain(
      'Codex, Claude and 2 more also work in this folder.',
    );
  });

  it('shows nothing extra when no other pane shares the folder', async () => {
    const sheet = await open([]);
    expect(sheet!.textContent).not.toContain('also work');
    expect(sheet!.textContent).not.toContain('also works');
  });
});

describe('panes sharing a folder', () => {
  const pane = (id: string, cwd: string | null) => ({ info: { id, cwd } });
  const names = new Map([
    ['a', 'Codex'],
    ['b', 'Claude'],
    ['c', 'Kilo'],
  ]);

  it('matches the same folder despite case and slash differences on Windows paths', () => {
    const panes = [
      pane('a', 'D:\\Work\\billing-service\\'),
      pane('b', 'd:/work/billing-service'),
      pane('c', 'D:/work/other'),
    ];
    expect(panesSharingFolder(panes, names, 'a')).toEqual(['Claude']);
    expect(folderKey('D:\\Work\\billing-service\\')).toBe(folderKey('d:/work/billing-service'));
  });

  it('returns nothing when no other pane has the folder, or the folder is unknown', () => {
    const panes = [pane('a', 'C:/x'), pane('b', 'C:/y'), pane('c', null)];
    expect(panesSharingFolder(panes, names, 'a')).toEqual([]);
    expect(panesSharingFolder(panes, names, 'c')).toEqual([]);
  });

  it('keeps case significant for non-Windows paths', () => {
    expect(folderKey('/home/Me/app')).not.toBe(folderKey('/home/me/app'));
    expect(folderKey('/home/me/app/')).toBe(folderKey('/home/me/app'));
  });

  it('builds no warning for an empty list', () => {
    expect(sharedFolderWarning([])).toBeNull();
  });
});

describe('History polish', () => {
  it('has one primary "Undo last turn" on top and the row Restore buttons are quiet (shown on hover or focus)', async () => {
    const api = mockApi({
      state: 'ok',
      message: '',
      items: [cp('9', 'After turn 2', 60_000, { kind: 'after', turn: 2 }), cp('8', 'Before turn 2', 10 * 60_000, { turn: 2 })],
    });
    await mount(api);
    const primary = Array.from(host.querySelectorAll<HTMLButtonElement>('button.ck-btn--primary'));
    expect(primary.map((b) => b.textContent)).toEqual(['Undo last turn']);
    const rows = Array.from(host.querySelectorAll('.ck-hist-row .ck-btn'));
    expect(rows.every((b) => b.classList.contains('ck-hist-restore') && !b.classList.contains('ck-btn--primary'))).toBe(true);
  });

  it('Undo last turn asks with its own title, then restores the start of the last turn', async () => {
    const api = mockApi({
      state: 'ok',
      message: '',
      items: [cp('9', 'After turn 2', 60_000, { kind: 'after', turn: 2 }), cp('8', 'Before turn 2', 10 * 60_000, { turn: 2 })],
    });
    await mount(api);
    await click(button('Undo last turn'));
    expect(host.querySelector('[role="alertdialog"]')!.textContent).toContain('Undo the last turn?');
    await click(Array.from(host.querySelectorAll('.ck-sheet button')).find((b) => b.textContent === 'Undo last turn'));
    expect(api.checkpointRestore).toHaveBeenCalledWith('p1', '8');
  });

  it('a prompt checkpoint names who asked: Before Claude Code\'s question', async () => {
    const api = mockApi({
      state: 'ok',
      message: '',
      items: [cp('6', 'Before answering a prompt (turn 3)', 90_000, { kind: 'answer', turn: 3 })],
    });
    await act(async () => {
      root.render(<History paneId="p1" status="idle" now={NOW} notify={vi.fn()} api={api} agentName="Claude Code" />);
    });
    expect(host.querySelector('.ck-hist-title')!.textContent).toBe("Before Claude Code's question (turn 3)");
    expect(host.textContent).not.toContain('a prompt');
  });

  it('timestamps are in the UI font (no mono class) and the row has no filled Restore', async () => {
    await mount(mockApi());
    expect(host.querySelector('.ck-hist-time')!.className).toBe('ck-hist-time');
    expect(host.querySelectorAll('.ck-hist-row .ck-btn--primary').length).toBe(0);
  });
});

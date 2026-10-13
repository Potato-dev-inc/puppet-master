// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mocked } from 'vitest';
import type { Checkpoint, CheckpointList, CheckpointsApi } from '../../lib/cockpit-api';
import { History } from './History';
import { checkpointTitle, formatAgo, formatDelta } from './history-model';

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
    expect(notify).toHaveBeenCalledWith(expect.stringContaining('Files restored to "Before turn 2"'));
    expect(api.checkpointsList.mock.calls.length).toBeGreaterThanOrEqual(2);
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

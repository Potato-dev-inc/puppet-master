// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi, type Mocked } from 'vitest';
import type { Checkpoint, CheckpointList, CheckpointsApi } from '../../lib/cockpit-api';
import { lastTurnStart } from './history-model';
import { UndoSheet } from './UndoSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function cp(id: string, turn: number, kind: string, label: string): Checkpoint {
  return { id, commit: `c${id}`, paneId: 'p1', turn, kind, label, note: '', atMs: 0, files: 1, added: 1, deleted: 0 };
}
// Newest first, like the backend returns them.
const ITEMS = [
  cp('4', 2, 'after', 'After turn 2'),
  cp('3', 2, 'before', 'Before turn 2'),
  cp('2', 1, 'after', 'After turn 1'),
  cp('1', 1, 'before', 'Before turn 1'),
];

function mockApi(listing: CheckpointList = { state: 'ok', message: '', items: ITEMS }) {
  return {
    checkpointsList: vi.fn(async () => listing),
    checkpointRestore: vi.fn(async () => ({ safetyId: '9', written: 1, removed: 0, failed: [] })),
  } as unknown as Mocked<Pick<CheckpointsApi, 'checkpointsList' | 'checkpointRestore'>>;
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
});

async function mount(api: ReturnType<typeof mockApi>, sharedWith: string[] = []) {
  const notify = vi.fn();
  const onClose = vi.fn();
  await act(async () => {
    root.render(<UndoSheet paneId="p1" sharedWith={sharedWith} notify={notify} onClose={onClose} api={api} />);
  });
  return { notify, onClose };
}
const button = (text: string) => Array.from(host.querySelectorAll('button')).find((b) => b.textContent?.trim() === text);

describe('lastTurnStart', () => {
  it('is the Before checkpoint of the highest turn and ignores undo checkpoints', () => {
    expect(lastTurnStart(ITEMS)?.id).toBe('3');
    expect(lastTurnStart([cp('9', 5, 'undo', 'Before undo'), ...ITEMS])?.id).toBe('3');
    expect(lastTurnStart([])).toBeUndefined();
    expect(lastTurnStart([cp('1', 1, 'after', 'After turn 1')])).toBeUndefined();
  });
});

describe('UndoSheet (palette: Undo last turn)', () => {
  it('opens the same confirm sheet, with the shared-folder warning, and restores nothing yet', async () => {
    const api = mockApi();
    await mount(api, ['Codex 2']);
    const sheet = host.querySelector('[role="alertdialog"]');
    expect(sheet).toBeTruthy();
    expect(sheet!.textContent).toContain('Undo the last turn?');
    expect(sheet!.textContent).toContain("It doesn't change what the agent remembers");
    expect(sheet!.textContent).toContain('Codex 2 also works in this folder');
    expect(api.checkpointRestore).not.toHaveBeenCalled();
  });

  it('restores the start of the last turn only after the confirm, then toasts with an Undo', async () => {
    const api = mockApi();
    const { notify, onClose } = await mount(api);
    await act(async () => button('Undo last turn')!.click());
    expect(api.checkpointRestore).toHaveBeenCalledExactlyOnceWith('p1', '3');
    expect(notify).toHaveBeenCalledWith('Files restored to "Before turn 2".', expect.objectContaining({ label: 'Undo' }));
    expect(onClose).toHaveBeenCalled();
  });

  it('Cancel restores nothing', async () => {
    const api = mockApi();
    const { onClose } = await mount(api);
    await act(async () => button('Cancel')!.click());
    expect(api.checkpointRestore).not.toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it('says so, and shows no sheet, when there is nothing to undo', async () => {
    const api = mockApi({ state: 'ok', message: '', items: [] });
    const { notify, onClose } = await mount(api);
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(notify).toHaveBeenCalledWith("There's nothing to undo yet.");
    expect(onClose).toHaveBeenCalled();
  });

  it('explains when checkpoints are off', async () => {
    const api = mockApi({ state: 'off', message: 'Checkpoints are off.', items: [] });
    const { notify } = await mount(api);
    expect(notify).toHaveBeenCalledWith('Checkpoints are off.');
  });
});

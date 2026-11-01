// @vitest-environment jsdom
// The Undo line after a delete: it names what was deleted, Undo calls back, and it goes away by itself after 10 s.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { UNDO_MS } from '../../../lib/potato-trash';
import { UndoToast } from './UndoToast';

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
  vi.useRealTimers();
});

describe('UndoToast', () => {
  it('shows the message and an Undo button that calls back', async () => {
    const onUndo = vi.fn();
    await act(async () => root.render(<UndoToast message="Note deleted." onUndo={onUndo} onExpire={() => {}} />));
    expect(host.querySelector('[role="status"]')?.textContent).toContain('Note deleted.');
    const undo = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Undo');
    await act(async () => undo!.click());
    expect(onUndo).toHaveBeenCalledTimes(1);
  });

  it('goes away by itself after 10 seconds, and a re-render does not restart the clock', async () => {
    vi.useFakeTimers();
    const onExpire = vi.fn();
    const show = (label: string) => <UndoToast message="Routine deleted." onUndo={() => {}} onExpire={() => onExpire(label)} />;
    await act(async () => root.render(show('first')));
    vi.advanceTimersByTime(UNDO_MS - 1000);
    await act(async () => root.render(show('second')));
    expect(onExpire).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1000);
    expect(onExpire).toHaveBeenCalledTimes(1);
    expect(onExpire).toHaveBeenCalledWith('second');
  });
});

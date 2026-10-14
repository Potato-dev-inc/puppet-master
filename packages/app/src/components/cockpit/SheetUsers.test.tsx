// @vitest-environment jsdom
// The real sheets (restore, panic) close with Escape: they used to ignore the keyboard.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { RestoreSheet } from './RestoreSheet';
import { PanicSheet } from './PanicSheet';

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

const escape = () =>
  act(() => {
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
  });

describe('sheets close with Escape', () => {
  it('RestoreSheet cancels, unless a restore is running', () => {
    const onCancel = vi.fn();
    act(() => root.render(<RestoreSheet title="Restore?" busy={false} onCancel={onCancel} onConfirm={() => {}} />));
    escape();
    expect(onCancel).toHaveBeenCalledTimes(1);

    act(() => root.render(<RestoreSheet title="Restore?" busy onCancel={onCancel} onConfirm={() => {}} />));
    escape();
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('PanicSheet closes', () => {
    const onClose = vi.fn();
    act(() => root.render(<PanicSheet report={{ stopped: [], failed: [] }} names={new Map()} onClose={onClose} />));
    escape();
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});

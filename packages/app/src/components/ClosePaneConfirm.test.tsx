// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { needsCloseConfirm } from '../lib/close-policy';
import { ClosePaneConfirm, type ClosePaneApi } from './ClosePaneConfirm';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mockApi() {
  let fire: (id: string) => void = () => undefined;
  const api = {
    onConfirmRequest: vi.fn(async (cb: (id: string) => void) => {
      fire = cb;
      return () => undefined;
    }),
    stopAndClose: vi.fn(async () => undefined),
  };
  return { api: api satisfies ClosePaneApi, fire: (id: string) => fire(id) };
}

let host: HTMLDivElement;
let root: Root;

async function settle() {
  await act(async () => {
    await Promise.resolve();
  });
}

const button = (label: string) =>
  [...host.querySelectorAll('button')].find((b) => b.textContent === label) as HTMLButtonElement | undefined;

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
});

describe('ClosePaneConfirm', () => {
  it('is hidden until Rust holds a close, then asks and stops on confirm', async () => {
    const { api, fire } = mockApi();
    await act(async () => root.render(<ClosePaneConfirm paneId="p1" agentLabel="Claude Code" api={api} />));
    await settle();
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();

    await act(async () => fire('p1'));
    expect(host.textContent).toContain('Claude Code is still working.');
    expect(host.textContent).toContain('Close and stop it?');

    await act(async () => button('Stop and close')!.click());
    expect(api.stopAndClose).toHaveBeenCalledWith('p1');
  });

  it('Keep open dismisses without stopping anything', async () => {
    const { api, fire } = mockApi();
    await act(async () => root.render(<ClosePaneConfirm paneId="p1" agentLabel="Codex" api={api} />));
    await settle();
    await act(async () => fire('p1'));
    await act(async () => button('Keep open')!.click());
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    expect(api.stopAndClose).not.toHaveBeenCalled();
  });

  it('ignores a request for another pane', async () => {
    const { api, fire } = mockApi();
    await act(async () => root.render(<ClosePaneConfirm paneId="p1" agentLabel="Codex" api={api} />));
    await settle();
    await act(async () => fire('other'));
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
  });
});

describe('needsCloseConfirm', () => {
  it('asks for working panes only, and respects the setting', () => {
    expect(needsCloseConfirm('running', true)).toBe(true);
    expect(needsCloseConfirm('waiting_input', undefined)).toBe(true);
    expect(needsCloseConfirm('running', false)).toBe(false);
    for (const s of ['idle', 'error', 'exited', undefined]) expect(needsCloseConfirm(s, true)).toBe(false);
  });
});

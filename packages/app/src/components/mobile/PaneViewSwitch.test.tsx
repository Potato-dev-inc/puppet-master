// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PaneViewSwitch } from './PaneViewSwitch';

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

describe('PaneViewSwitch', () => {
  it('shows Terminal and Chat with the open one pressed', () => {
    act(() => root.render(<PaneViewSwitch view="chat" onChange={() => undefined} />));
    const buttons = Array.from(host.querySelectorAll('button'));
    expect(buttons.map((b) => b.textContent)).toEqual(['Terminal', 'Chat']);
    expect(buttons.map((b) => b.getAttribute('aria-pressed'))).toEqual(['false', 'true']);
    expect(host.querySelector('[role="group"]')?.getAttribute('aria-label')).toBe('Pane view');
  });

  it('tells which view was picked', () => {
    const onChange = vi.fn();
    act(() => root.render(<PaneViewSwitch view="terminal" onChange={onChange} />));
    const [terminal, chat] = Array.from(host.querySelectorAll('button'));
    act(() => chat!.click());
    expect(onChange).toHaveBeenLastCalledWith('chat');
    act(() => terminal!.click());
    expect(onChange).toHaveBeenLastCalledWith('terminal');
  });
});

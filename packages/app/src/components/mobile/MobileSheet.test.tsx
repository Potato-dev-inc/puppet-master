// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { describe, expect, it, vi } from 'vitest';
import { MenuSheet } from './MobileSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

function mount(onStopAll: () => void) {
  const host = document.createElement('div');
  document.body.appendChild(host);
  act(() =>
    createRoot(host).render(
      <MenuSheet
        onClose={() => {}}
        bridgeUrl="http://x"
        bridgeReady
        settings={{} as never}
        onPatchSettings={() => {}}
        link={null}
        echo={null}
        activeLabel={null}
        onCloseActive={() => {}}
        onEditConnection={() => {}}
        onStopAll={onStopAll}
      />,
    ),
  );
  return host;
}
const stopButton = (host: HTMLElement) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent === 'Stop all agents') as HTMLButtonElement;

describe('phone menu: Stop all agents', () => {
  it('asks for confirmation before stopping anything', () => {
    const stop = vi.fn();
    const host = mount(stop);
    const confirm = vi.spyOn(window, 'confirm');
    confirm.mockReturnValueOnce(false);
    act(() => stopButton(host).click());
    expect(stop).not.toHaveBeenCalled();
    confirm.mockReturnValueOnce(true);
    act(() => stopButton(host).click());
    expect(stop).toHaveBeenCalledTimes(1);
  });
});

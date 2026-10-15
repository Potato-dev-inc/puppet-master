// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MenuSheet, NewAgentSheet, Sheet } from './MobileSheet';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.innerHTML = '';
  vi.useRealTimers();
});

function mount(node: React.ReactNode) {
  const host = document.createElement('div');
  document.body.appendChild(host);
  act(() => createRoot(host).render(node));
  return host;
}

function mountMenu(onStopAll: () => void, onCloseActive = () => {}, activeLabel: string | null = null) {
  return mount(
    <MenuSheet
      onClose={() => {}}
      bridgeUrl="http://x"
      bridgeReady
      settings={{} as never}
      onPatchSettings={() => {}}
      link={null}
      echo={null}
      activeLabel={activeLabel}
      onCloseActive={onCloseActive}
      onEditConnection={() => {}}
      onStopAll={onStopAll}
    />,
  );
}
const button = (host: HTMLElement, text: string) =>
  Array.from(host.querySelectorAll('button')).find((b) => b.textContent === text) as HTMLButtonElement;

describe('phone menu: Stop all agents', () => {
  it('asks in an action sheet before stopping anything', () => {
    const stop = vi.fn();
    const host = mountMenu(stop);
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    act(() => button(host, 'Stop all agents').click());
    // Nothing stopped yet: the confirmation is showing.
    expect(stop).not.toHaveBeenCalled();
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('Stop all agents?');
    // Cancel keeps everything running.
    act(() => button(host, 'Cancel').click());
    expect(stop).not.toHaveBeenCalled();
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
    // Confirm stops.
    act(() => button(host, 'Stop all agents').click());
    act(() => button(host, 'Stop all').click());
    expect(stop).toHaveBeenCalledTimes(1);
    expect(host.querySelector('[role="alertdialog"]')).toBeNull();
  });

  it('closing the open agent is confirmed the same way', () => {
    const close = vi.fn();
    const host = mountMenu(() => {}, close, 'Fix login');
    act(() => button(host, 'Close Fix login').click());
    expect(close).not.toHaveBeenCalled();
    expect(host.querySelector('[role="alertdialog"]')?.textContent).toContain('Close Fix login?');
    const confirmButtons = Array.from(host.querySelectorAll('button')).filter((b) => b.textContent === 'Close Fix login');
    expect(confirmButtons.length).toBe(2); // the menu row and the sheet's confirm action
    act(() => confirmButtons[1].click());
    expect(close).toHaveBeenCalledTimes(1);
  });

  it('has switches and 44px-class rows, no Tailwind leftovers', () => {
    const host = mountMenu(() => {});
    expect(host.querySelectorAll('input.ph-switch[type="checkbox"]').length).toBeGreaterThanOrEqual(3);
    expect(host.innerHTML).not.toMatch(/\bpm-(border|panel|raised)\b/);
  });
});

describe('sheets', () => {
  it('has a grabber and a Done button that closes', () => {
    const onClose = vi.fn();
    const host = mount(
      <Sheet title="Example" onClose={onClose}>
        body
      </Sheet>,
    );
    expect(host.querySelector('.ph-grabber')).not.toBeNull();
    act(() => button(host, 'Done').click());
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('closes on Escape and on a tap of the scrim', () => {
    const onClose = vi.fn();
    const host = mount(
      <Sheet title="Example" onClose={onClose}>
        body
      </Sheet>,
    );
    act(() => {
      window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    });
    expect(onClose).toHaveBeenCalledTimes(1);
    act(() => (host.querySelector('.ph-scrim') as HTMLElement).click());
    expect(onClose).toHaveBeenCalledTimes(2);
  });

  const drag = (host: HTMLElement, fromY: number, toY: number) => {
    const top = host.querySelector('.ph-sheet-top') as HTMLElement;
    const ev = (type: string, y: number) =>
      new MouseEvent(type, { bubbles: true, clientY: y }) as MouseEvent;
    // Events 100ms apart, so the speed stays well under a fling.
    const step = (e: MouseEvent) => {
      act(() => {
        top.dispatchEvent(e);
        vi.advanceTimersByTime(100);
      });
    };
    step(ev('pointerdown', fromY));
    step(ev('pointermove', (fromY + toY) / 2));
    step(ev('pointermove', toY));
    step(ev('pointerup', toY));
  };

  it('swipe down past the threshold closes the sheet; a short pull springs back', () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    const host = mount(
      <Sheet title="Example" onClose={onClose}>
        body
      </Sheet>,
    );
    drag(host, 100, 130); // 30px: springs back
    act(() => vi.advanceTimersByTime(400));
    expect(onClose).not.toHaveBeenCalled();
    expect((host.querySelector('.ph-sheet') as HTMLElement).style.transform).toBe('');
    drag(host, 100, 320); // 220px: closes after the slide-out
    expect(onClose).not.toHaveBeenCalled();
    act(() => vi.advanceTimersByTime(400));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('New agent sheet lists agents as rows; disabled agents show why', () => {
    const onSpawn = vi.fn();
    const host = mount(
      <NewAgentSheet
        entries={[
          { type: 'claude', label: 'Claude Code', icon: null },
          { type: 'cursor_agent', label: 'Cursor Agent', icon: null, disabled: true, disabledReason: 'Not installed' },
        ]}
        onClose={() => {}}
        onSpawn={onSpawn}
        spawning={false}
        error={null}
      />,
    );
    const rows = host.querySelectorAll('.ph-row');
    expect(rows.length).toBe(2);
    expect(rows[1].textContent).toContain('Not installed');
    expect((rows[1] as HTMLButtonElement).disabled).toBe(true);
    act(() => (rows[0] as HTMLButtonElement).click());
    expect(onSpawn).toHaveBeenCalledWith('claude');
  });
});

// @vitest-environment jsdom
import { act } from 'react';
import { createRoot } from 'react-dom/client';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MenuSheet, NewAgentSheet, Sheet } from './MobileSheet';

// The Telegram row talks to the PC bridge; the tests answer for it. Nothing reaches a real bridge.
const telegram = vi.hoisted(() => ({
  status: vi.fn(async () => ({ on: false, connected: false, botUsername: null as string | null })),
  turnOff: vi.fn(async () => ({ ok: true, wasOn: true })),
}));
vi.mock('../../lib/telegram-bridge', () => ({
  getTelegramPhoneStatus: telegram.status,
  turnTelegramOff: telegram.turnOff,
}));

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

  it('opens "Your plans" from the menu', () => {
    const open = vi.fn();
    const host = mount(
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
        onOpenPlans={open}
      />,
    );
    act(() => button(host, 'Your plans').click());
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('has no Your plans row when the menu is given no way to open it', () => {
    const host = mountMenu(() => {});
    expect(button(host, 'Your plans')).toBeUndefined();
  });

  it('has switches and 44px-class rows, no Tailwind leftovers', () => {
    const host = mountMenu(() => {});
    expect(host.querySelectorAll('input.ph-switch[type="checkbox"]').length).toBeGreaterThanOrEqual(3);
    expect(host.innerHTML).not.toMatch(/\bpm-(border|panel|raised)\b/);
  });
});

describe('phone menu: Telegram off row', () => {
  const h2 = (host: HTMLElement, text: string) => Array.from(host.querySelectorAll('h2')).find((h) => h.textContent === text) as HTMLElement;

  it('is hidden while the PC says Telegram is off', async () => {
    telegram.status.mockResolvedValueOnce({ on: false, connected: false, botUsername: null });
    const host = mountMenu(() => {});
    await act(async () => {});
    expect(button(host, 'Turn Telegram off')).toBeUndefined();
    expect(host.textContent).not.toContain('Telegram ·');
  });

  it('sits under Notifications and before Terminal when Telegram is on', async () => {
    telegram.status.mockResolvedValueOnce({ on: true, connected: true, botUsername: 'pm_bot' });
    const host = mountMenu(() => {});
    await act(async () => {});
    const turnOff = button(host, 'Turn Telegram off');
    expect(turnOff).toBeDefined();
    const notifications = h2(host, 'Notifications');
    const terminal = h2(host, 'Terminal');
    expect(notifications.compareDocumentPosition(turnOff) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(turnOff.compareDocumentPosition(terminal) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('turning it off from the phone says so, and the PC is the only place to turn it back on', async () => {
    telegram.status.mockResolvedValueOnce({ on: true, connected: true, botUsername: 'pm_bot' });
    const host = mountMenu(() => {});
    await act(async () => {});
    await act(async () => button(host, 'Turn Telegram off').click());
    expect(telegram.turnOff).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain('Off. Turn it on again on your PC.');
    expect(button(host, 'Turn Telegram off')).toBeUndefined();
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

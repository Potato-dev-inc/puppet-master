// @vitest-environment jsdom
// The Mac permission note: plain words about what to switch on and where, only when it is switched off, and asked
// again when the person comes back from System Settings. The command is mocked; nothing here touches an OS.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { PermissionBanner } from './PermissionBanner';
import { accessibilityHelp, useDeskPermissions } from '../../lib/desk-permissions';

const h = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/core', () => ({ invoke: h.invoke }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  h.invoke.mockReset();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

const flush = () => act(async () => {});

function Harness({ enabled = true }: { enabled?: boolean }) {
  const { permissions, recheck } = useDeskPermissions(enabled);
  return <PermissionBanner permissions={permissions} onRecheck={recheck} />;
}

describe('accessibilityHelp', () => {
  it('has words only when Accessibility is off', () => {
    expect(accessibilityHelp({ accessibility: true })).toBeNull();
    expect(accessibilityHelp({ accessibility: null })).toBeNull();
    expect(accessibilityHelp(null)).toBeNull();
    const help = accessibilityHelp({ accessibility: false })!;
    expect(help.title).toContain('has not allowed Puppet Master');
    expect(help.steps).toContain('System Settings');
    expect(help.steps).toContain('Privacy & Security');
    expect(help.steps).toContain('Accessibility');
  });
});

describe('PermissionBanner', () => {
  it('says what to switch on and where when the command says false', async () => {
    h.invoke.mockResolvedValue({ accessibility: false });
    act(() => root.render(<Harness />));
    await flush();
    expect(h.invoke).toHaveBeenCalledWith('desk_permissions');
    const banner = host.querySelector('[data-testid="permission-banner"]')!;
    expect(banner.getAttribute('role')).toBe('alert');
    expect(banner.textContent).toContain('System Settings');
    expect(banner.textContent).toContain('Privacy & Security');
    expect(banner.textContent).toContain('Accessibility');
    expect(banner.textContent).toContain('switch on Puppet Master');
  });

  it('shows nothing when Accessibility is on, not applicable, or the command fails', async () => {
    for (const answer of [{ accessibility: true }, { accessibility: null }]) {
      h.invoke.mockResolvedValue(answer);
      act(() => root.render(<Harness />));
      await flush();
      expect(host.textContent).toBe('');
    }
    h.invoke.mockRejectedValue('FEATURE_OFF: My computers is switched off in Labs');
    act(() => root.render(<Harness enabled={false} />));
    act(() => root.render(<Harness />));
    await flush();
    expect(host.textContent).toBe('');
    h.invoke.mockResolvedValue('not an answer');
    act(() => root.render(<Harness enabled={false} />));
    act(() => root.render(<Harness />));
    await flush();
    expect(host.textContent).toBe('');
  });

  it('asks nothing until enabled', async () => {
    h.invoke.mockResolvedValue({ accessibility: false });
    act(() => root.render(<Harness enabled={false} />));
    await flush();
    expect(h.invoke).not.toHaveBeenCalled();
  });

  it('asks again when the person comes back to the window, and the note goes once they switched it on', async () => {
    h.invoke.mockResolvedValueOnce({ accessibility: false });
    act(() => root.render(<Harness />));
    await flush();
    expect(host.querySelector('[data-testid="permission-banner"]')).not.toBeNull();
    h.invoke.mockResolvedValueOnce({ accessibility: true });
    await act(async () => {
      window.dispatchEvent(new Event('focus'));
    });
    expect(h.invoke).toHaveBeenCalledTimes(2);
    expect(host.querySelector('[data-testid="permission-banner"]')).toBeNull();
  });

  it('"Check again" asks again', async () => {
    h.invoke.mockResolvedValue({ accessibility: false });
    act(() => root.render(<Harness />));
    await flush();
    await act(async () => host.querySelector<HTMLButtonElement>('button')!.click());
    expect(h.invoke).toHaveBeenCalledTimes(2);
  });
});

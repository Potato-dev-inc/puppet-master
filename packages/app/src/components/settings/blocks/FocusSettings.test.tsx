// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { focusSettingsSet, focusState, type FocusSettings, type FocusView } from '../../../lib/focus';
import { clampFocusMinutes, FocusSettingsBlock } from './FocusSettings';

vi.mock('../../../lib/focus', async (importOriginal) => {
  const orig = await importOriginal<typeof import('../../../lib/focus')>();
  return { ...orig, focusState: vi.fn(), focusSettingsSet: vi.fn() };
});

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const DEFAULTS: FocusSettings = { default_minutes: 45, quiet_blocking: true };

function viewWith(settings: FocusSettings): FocusView {
  return {
    on: false,
    sessionId: null,
    startedAtMs: null,
    endsAtMs: null,
    minutes: 0,
    extended: 0,
    held: { run_done: 0, run_failed: 0, companion: 0, review: 0, night: 0 },
    heldTotal: 0,
    waitingQuiet: 0,
    waitingUrgent: 0,
    settings,
    lastSummary: null,
  };
}

let host: HTMLDivElement;
let root: Root;
beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
  vi.mocked(focusState).mockResolvedValue(viewWith(DEFAULTS));
  vi.mocked(focusSettingsSet).mockImplementation(async (next: FocusSettings) => viewWith(next));
});
afterEach(() => {
  act(() => root.unmount());
  host.remove();
  vi.clearAllMocks();
});

async function flush() {
  await act(async () => {
    await new Promise((r) => setTimeout(r, 0));
  });
}

async function mount(onSaved = vi.fn()) {
  await act(async () => {
    root.render(<FocusSettingsBlock onSaved={onSaved} />);
  });
  await flush();
  return { onSaved };
}

const lengthField = () => host.querySelector<HTMLInputElement>('[aria-label="Default length (minutes)"]')!;
const quietSwitch = () => host.querySelector<HTMLButtonElement>('[role="switch"][aria-label="Make cards that wait for you quiet during Focus"]')!;

/** Types into a React-controlled number field, the way a person would (no save yet). */
async function type(el: HTMLInputElement, value: string) {
  await act(async () => {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, value);
    el.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

async function blur(el: HTMLInputElement) {
  await act(async () => {
    el.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
  });
  await flush();
}

async function click(el: HTMLElement) {
  await act(async () => {
    el.click();
  });
  await flush();
}

describe('clampFocusMinutes', () => {
  it.each([
    [2, 5],
    [5, 5],
    [45.4, 45],
    [45.6, 46],
    [240, 240],
    [999, 240],
    [-3, 5],
    [Number.NaN, 45],
    ['90', 45],
  ])('%s becomes %s', (input, expected) => {
    expect(clampFocusMinutes(input)).toBe(expected);
  });
});

describe('FocusSettingsBlock', () => {
  it('shows the saved default length and the quiet switch', async () => {
    await mount();
    expect(lengthField().value).toBe('45');
    expect(quietSwitch().getAttribute('aria-checked')).toBe('true');
  });

  it('renders nothing outside the desktop shell, where there is no Focus to configure', async () => {
    vi.mocked(focusState).mockRejectedValueOnce(new Error('no tauri'));
    await mount();
    expect(host.textContent).toBe('');
  });

  it('clamps the default length to 5..240 on blur and saves only then', async () => {
    const { onSaved } = await mount();
    await type(lengthField(), '2');
    expect(focusSettingsSet).not.toHaveBeenCalled();
    await blur(lengthField());
    expect(focusSettingsSet).toHaveBeenCalledExactlyOnceWith({ default_minutes: 5, quiet_blocking: true });
    expect(lengthField().value).toBe('5');

    await type(lengthField(), '9000');
    await blur(lengthField());
    expect(focusSettingsSet).toHaveBeenLastCalledWith({ default_minutes: 240, quiet_blocking: true });
    expect(onSaved).toHaveBeenCalledTimes(2);
  });

  it('saves nothing when the typed length is the saved one', async () => {
    await mount();
    await type(lengthField(), '45');
    await blur(lengthField());
    expect(focusSettingsSet).not.toHaveBeenCalled();
  });

  it('the quiet switch saves through focusSettingsSet with the length kept', async () => {
    const { onSaved } = await mount();
    await click(quietSwitch());
    expect(focusSettingsSet).toHaveBeenCalledExactlyOnceWith({ default_minutes: 45, quiet_blocking: false });
    expect(quietSwitch().getAttribute('aria-checked')).toBe('false');
    expect(onSaved).toHaveBeenCalledTimes(1);
    await click(quietSwitch());
    expect(focusSettingsSet).toHaveBeenLastCalledWith({ default_minutes: 45, quiet_blocking: true });
  });

  it('says plainly that cards that wait for you are never held, and what still comes through', async () => {
    await mount();
    expect(host.textContent).toContain('Cards that wait for you are never held.');
    expect(host.textContent).toContain('They still show up in the Inbox.');
    expect(host.textContent).toContain('After 20 minutes you get one reminder.');
    expect(host.textContent).toContain('Urgent things always come through: a stuck agent, a spent budget, a high-risk request, or an agent asking for more control.');
    expect(host.textContent).toContain('Focus never answers anything for you.');
  });

  it('uses no word for muting in the copy', async () => {
    await mount();
    expect(host.textContent ?? '').not.toMatch(/mute|\bDND\b|disturb|suppress|notifications off/i);
  });

  it('a refused save shows the reason, keeps the saved value and does not count as saved', async () => {
    vi.mocked(focusSettingsSet).mockRejectedValueOnce(new Error('Focus is busy'));
    const { onSaved } = await mount();
    await click(quietSwitch());
    expect(host.querySelector('[role="alert"]')?.textContent).toBe('Not saved: Focus is busy');
    expect(quietSwitch().getAttribute('aria-checked')).toBe('true');
    expect(onSaved).not.toHaveBeenCalled();
  });
});

// @vitest-environment jsdom
// Autosave behaviour of the settings controls: debounce for text, clamp-on-blur for numbers, blur-only for secrets.
import { act, type ReactNode } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ConfirmSheet, NumberControl, Popup, SecretControl, Switch, TextControl } from './controls';
import { blur, click, pickOption, type as typeInto } from './test-utils';

let host: HTMLDivElement;
let root: Root;

async function render(node: ReactNode) {
  await act(async () => {
    root.render(node);
  });
}
const input = () => host.querySelector('input') as HTMLInputElement;

beforeEach(() => {
  vi.useFakeTimers();
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});
afterEach(async () => {
  await act(async () => root.unmount());
  host.remove();
  vi.useRealTimers();
});

describe('Switch and Popup save immediately', () => {
  it('a switch reports the flipped value on click', async () => {
    const onChange = vi.fn();
    await render(<Switch checked={false} onChange={onChange} label="Checkpoints" />);
    const sw = host.querySelector('[role="switch"]') as HTMLButtonElement;
    expect(sw.getAttribute('aria-checked')).toBe('false');
    await click(sw);
    expect(onChange).toHaveBeenCalledExactlyOnceWith(true);
  });

  it('a popup reports the picked value on change', async () => {
    const onChange = vi.fn();
    await render(
      <Popup aria-label="Theme" value="dark" onChange={(e) => onChange(e.target.value)}>
        <option value="dark">Dark</option>
        <option value="light">Light</option>
      </Popup>,
    );
    await pickOption(host.querySelector('select') as HTMLSelectElement, 'light');
    expect(onChange).toHaveBeenCalledExactlyOnceWith('light');
  });
});

describe('TextControl', () => {
  it('debounces: nothing is saved while typing, one save after the pause', async () => {
    const onCommit = vi.fn();
    await render(<TextControl value="" onCommit={onCommit} debounceMs={600} aria-label="Folder" />);
    await typeInto(input(), 'C:\\wo');
    await typeInto(input(), 'C:\\work');
    await act(async () => void vi.advanceTimersByTime(599));
    expect(onCommit).not.toHaveBeenCalled();
    await act(async () => void vi.advanceTimersByTime(2));
    expect(onCommit).toHaveBeenCalledExactlyOnceWith('C:\\work');
  });

  it('blur saves right away without waiting for the pause', async () => {
    const onCommit = vi.fn();
    await render(<TextControl value="a" onCommit={onCommit} />);
    await typeInto(input(), 'ab');
    await blur(input());
    expect(onCommit).toHaveBeenCalledExactlyOnceWith('ab');
    await act(async () => void vi.advanceTimersByTime(5000));
    expect(onCommit).toHaveBeenCalledTimes(1); // the pending timer was cancelled by the flush
  });

  it('mode "blur" never saves on a timer', async () => {
    const onCommit = vi.fn();
    await render(<TextControl value="" onCommit={onCommit} mode="blur" />);
    await typeInto(input(), 'secret-ish');
    await act(async () => void vi.advanceTimersByTime(10_000));
    expect(onCommit).not.toHaveBeenCalled();
    await blur(input());
    expect(onCommit).toHaveBeenCalledExactlyOnceWith('secret-ish');
  });

  it('does not save an unchanged value, and applies normalize (trim) before saving', async () => {
    const onCommit = vi.fn();
    await render(<TextControl value="abc" onCommit={onCommit} normalize={(s) => s.trim()} />);
    await typeInto(input(), '  abc  ');
    await blur(input());
    expect(onCommit).not.toHaveBeenCalled();
    await typeInto(input(), '  abcd ');
    await blur(input());
    expect(onCommit).toHaveBeenCalledExactlyOnceWith('abcd');
  });

  it('saves a pending edit when it unmounts (leaving the section does not lose text)', async () => {
    const onCommit = vi.fn();
    await render(<TextControl value="" onCommit={onCommit} />);
    await typeInto(input(), 'half-typed');
    await render(<div />);
    expect(onCommit).toHaveBeenCalledExactlyOnceWith('half-typed');
  });

  it('follows the saved value when it changes elsewhere, but not while you are typing', async () => {
    await render(<TextControl value="one" onCommit={vi.fn()} />);
    expect(input().value).toBe('one');
    await render(<TextControl value="two" onCommit={vi.fn()} />);
    expect(input().value).toBe('two');
    await typeInto(input(), 'mine');
    await render(<TextControl value="three" onCommit={vi.fn()} />);
    expect(input().value).toBe('mine');
  });
});

describe('NumberControl', () => {
  it('does not clamp or save while typing; clamps and saves on blur', async () => {
    const onCommit = vi.fn();
    await render(<NumberControl value={50} min={5} max={500} onCommit={onCommit} />);
    await typeInto(input(), '1');
    expect(input().value).toBe('1'); // typing "1" on the way to "120" must not snap to 5
    expect(onCommit).not.toHaveBeenCalled();
    await typeInto(input(), '12');
    await typeInto(input(), '120');
    await blur(input());
    expect(onCommit).toHaveBeenCalledExactlyOnceWith(120);
  });

  it('clamps out-of-range numbers on blur and shows the clamped value', async () => {
    const onCommit = vi.fn();
    await render(<NumberControl value={50} min={5} max={500} onCommit={onCommit} />);
    await typeInto(input(), '9999');
    await blur(input());
    expect(onCommit).toHaveBeenCalledExactlyOnceWith(500);
    expect(input().value).toBe('500');
    await typeInto(input(), '1');
    await blur(input());
    expect(onCommit).toHaveBeenLastCalledWith(5);
  });

  it('puts the saved value back when the text is not a number, and saves nothing', async () => {
    const onCommit = vi.fn();
    await render(<NumberControl value={50} min={5} max={500} onCommit={onCommit} />);
    await typeInto(input(), '');
    await blur(input());
    expect(onCommit).not.toHaveBeenCalled();
    expect(input().value).toBe('50');
  });

  it('uses a custom normalize and Enter commits', async () => {
    const onCommit = vi.fn();
    await render(<NumberControl value={0} onCommit={onCommit} normalize={(n) => (n <= 0 ? 0 : Math.min(1000, Math.max(50, n)))} />);
    await typeInto(input(), '10');
    await act(async () => {
      input().dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    });
    expect(onCommit).toHaveBeenCalledExactlyOnceWith(50);
  });
});

describe('SecretControl', () => {
  it('saves only on blur, then clears and never shows the secret', async () => {
    const onCommit = vi.fn();
    await render(<SecretControl aria-label="Key A" onCommit={onCommit} />);
    expect(input().type).toBe('password');
    await typeInto(input(), 'sk-live-123');
    await act(async () => void vi.advanceTimersByTime(60_000));
    expect(onCommit).not.toHaveBeenCalled();
    await blur(input());
    expect(onCommit).toHaveBeenCalledExactlyOnceWith('sk-live-123');
    expect(input().value).toBe('');
  });

  it('an empty field saves nothing', async () => {
    const onCommit = vi.fn();
    await render(<SecretControl onCommit={onCommit} />);
    await typeInto(input(), '   ');
    await blur(input());
    expect(onCommit).not.toHaveBeenCalled();
  });
});

describe('ConfirmSheet', () => {
  it('focuses Cancel, so Enter never confirms; Escape cancels', async () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    await render(<ConfirmSheet title="Remove all keys?" body="Agents lose access." confirmLabel="Remove" onConfirm={onConfirm} onCancel={onCancel} />);
    expect(document.activeElement?.textContent).toBe('Cancel');
    const dialog = host.querySelector('[role="dialog"]') as HTMLElement;
    expect(dialog.textContent).toContain('Remove all keys?');
    await act(async () => {
      host.querySelector('.st-scrim')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    });
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
    const confirmBtn = [...host.querySelectorAll('button')].find((b) => b.textContent === 'Remove')!;
    await click(confirmBtn);
    expect(onConfirm).toHaveBeenCalledTimes(1);
  });
});

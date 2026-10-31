// @vitest-environment jsdom
// The small parts: the mood chip (text only), the voice switch, and the typing row.
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { MoodChip } from './MoodChip';
import { PersonaSwitch } from './PersonaSwitch';
import { TypingRow } from './TypingRow';
import { buttonByText } from './test-helpers';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
(globalThis as { ResizeObserver?: unknown }).ResizeObserver ??= class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

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

describe('MoodChip', () => {
  it('says the mood and the energy in words and draws nothing else', () => {
    act(() => root.render(<MoodChip mood={{ mood: 'genki', intensity: 3, energy: 72.4, hunger: 10 }} />));
    expect(host.textContent).toBe('Genki · energy 72');
    expect(host.querySelector('svg, img')).toBeNull();
  });

  it('draws nothing before the bot has a mood', () => {
    act(() => root.render(<MoodChip mood={null} />));
    expect(host.textContent).toBe('');
  });
});

describe('PersonaSwitch', () => {
  it('lists Fish and Potato and marks the current voice', () => {
    act(() => root.render(<PersonaSwitch value="fish" onChange={() => {}} />));
    const tabs = [...host.querySelectorAll('[role="tab"]')];
    expect(tabs.map((t) => t.textContent)).toEqual(['Fish', 'Potato']);
    expect(tabs.map((t) => t.getAttribute('aria-selected'))).toEqual(['true', 'false']);
  });

  it('asks for the other voice when it is clicked', () => {
    const onChange = vi.fn();
    act(() => root.render(<PersonaSwitch value="fish" onChange={onChange} />));
    act(() => buttonByText(host, 'Potato')!.click());
    expect(onChange).toHaveBeenCalledWith('potato');
  });
});

describe('TypingRow', () => {
  it('names the bot and shows the running dots', () => {
    act(() => root.render(<TypingRow name="Fish" />));
    expect(host.textContent).toBe('Fish is typing…');
    const dots = host.querySelector('.pm-dots');
    expect(dots).not.toBeNull();
    expect(dots?.querySelectorAll('.pm-dots-sweep').length).toBeGreaterThan(0);
  });
});

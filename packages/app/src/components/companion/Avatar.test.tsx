// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AvatarId, CompanionState } from '../../lib/companion';
import { AVATARS } from './avatars';
import { Avatar, BLINK_MIN_MS, BLINK_MS, SMALL_PX } from './Avatar';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const IDS: AvatarId[] = ['mochi', 'bit', 'nib', 'tofu', 'lumen', 'hoot', 'sprout', 'kit'];
const STATES: CompanionState[] = ['starting', 'idle', 'working', 'needs_you', 'sleeping', 'stopped'];

let host: HTMLDivElement;
let root: Root;

function render(props: { id: AvatarId; size?: number; state: CompanionState; name?: string }): SVGSVGElement {
  act(() => {
    root.render(<Avatar id={props.id} size={props.size ?? 96} state={props.state} name={props.name} />);
  });
  return host.querySelector('svg') as unknown as SVGSVGElement;
}

/** The creature's dots (the first group) and the overlay's dots (the second group, when there is one). */
const bodyDots = (svg: SVGSVGElement): SVGCircleElement[] => Array.from(svg.children[0]!.querySelectorAll('circle'));
const overlayDots = (svg: SVGSVGElement): SVGCircleElement[] =>
  svg.children[1] ? Array.from(svg.children[1].querySelectorAll('circle')) : [];
const count = (el: Element, selector: string): number => el.querySelectorAll(selector).length;
const cellCount = (id: AvatarId, ch: string): number => AVATARS[id].rows.join('').split(ch).length - 1;

/** A reduced-motion preference, or none at all. */
function stubReducedMotion(matches: boolean): void {
  Object.defineProperty(window, 'matchMedia', {
    configurable: true,
    writable: true,
    value: (query: string) => ({
      matches,
      media: query,
      addEventListener: () => undefined,
      removeEventListener: () => undefined,
      addListener: () => undefined,
      removeListener: () => undefined,
      onchange: null,
      dispatchEvent: () => false,
    }),
  });
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
  delete (window as { matchMedia?: unknown }).matchMedia;
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe('Avatar label and size', () => {
  it('says the face name and the state word', () => {
    expect(render({ id: 'mochi', state: 'idle' }).getAttribute('aria-label')).toBe('Mochi, Here');
  });

  it('uses the companion name when it has one', () => {
    expect(render({ id: 'kit', state: 'needs_you', name: 'Bun' }).getAttribute('aria-label')).toBe('Bun, Needs you');
  });

  it('draws at the given width, 15 to 12 high', () => {
    const svg = render({ id: 'nib', size: 150, state: 'idle' });
    expect(svg.getAttribute('width')).toBe('150');
    expect(svg.getAttribute('height')).toBe('120');
    expect(svg.getAttribute('viewBox')).toBe('0 0 15 12');
  });

  it.each(IDS.flatMap((id) => STATES.map((state) => [id, state] as const)))('draws %s when %s', (id, state) => {
    const svg = render({ id, state });
    expect(bodyDots(svg).length).toBeGreaterThan(0);
  });
});

describe('Avatar overlays', () => {
  it('draws no overlay when idle', () => {
    const svg = render({ id: 'mochi', state: 'idle' });
    expect(svg.children).toHaveLength(1);
  });

  it('draws three ripple dots when working, staggered 300 ms apart', () => {
    const dots = overlayDots(render({ id: 'bit', state: 'working' }));
    expect(dots).toHaveLength(3);
    expect(dots.map((d) => d.getAttribute('class'))).toEqual(['ca-ripple ca-move-ripple', 'ca-ripple ca-move-ripple', 'ca-ripple ca-move-ripple']);
    expect(dots.map((d) => d.style.animationDelay)).toEqual(['0ms', '-300ms', '-600ms']);
    expect(dots.map((d) => d.getAttribute('cx'))).toEqual(['12.5', '13.5', '14.5']);
  });

  it('draws the exclamation mark when it needs you, and lifts the body', () => {
    const svg = render({ id: 'hoot', state: 'needs_you' });
    const bang = overlayDots(svg);
    expect(bang.map((d) => d.getAttribute('cx'))).toEqual(['13.5', '13.5', '13.5', '13.5']);
    expect(bang.map((d) => d.getAttribute('cy'))).toEqual(['0.5', '1.5', '2.5', '4.5']);
    expect(svg.children[0]!.getAttribute('class')).toBe('ca-move-hop');
  });

  it('draws seven z cells when sleeping, with the body dimmed and the eyes closed', () => {
    const svg = render({ id: 'mochi', state: 'sleeping' });
    expect(overlayDots(svg)).toHaveLength(7);
    expect(count(svg, '.ca-z.ca-move-z')).toBe(7);
    expect(svg.children[0]!.getAttribute('class')).toBe('ca-body-sleep');
    // Every eye cell is drawn as a dim dot: the plus cells plus the eye cells.
    expect(count(svg, '.ca-dim')).toBe(cellCount('mochi', '+') + cellCount('mochi', 'e'));
  });

  it('shows the stopped face faded, with the eyes closed and no overlay', () => {
    const svg = render({ id: 'tofu', state: 'stopped' });
    expect(svg.getAttribute('class')).toContain('ca-stopped');
    expect(svg.children).toHaveLength(1);
    expect(count(svg, '.ca-dim')).toBe(cellCount('tofu', '+') + cellCount('tofu', 'e'));
  });

  it('draws the starting rows bottom to top, 40 ms apart', () => {
    const svg = render({ id: 'mochi', state: 'starting' });
    const bottom = bodyDots(svg).find((d) => d.getAttribute('cy') === '10.5');
    const top = bodyDots(svg).find((d) => d.getAttribute('cy') === '2.5');
    expect(bottom?.style.animationDelay).toBe('40ms');
    expect(top?.style.animationDelay).toBe('360ms');
    expect(count(svg, '.ca-move-row')).toBe(bodyDots(svg).length);
  });

  it('draws a dot-eye face with its eyes open as full dots', () => {
    const svg = render({ id: 'bit', state: 'idle' });
    expect(count(svg, '.ca-dim')).toBe(cellCount('bit', '+'));
    // Bit has 4 eye cells, all drawn as full dots when open.
    expect(bodyDots(svg).length).toBe(cellCount('bit', '#') + cellCount('bit', 'e') + cellCount('bit', '+'));
  });
});

describe('Avatar small size', () => {
  it('draws dim dots solid at half opacity below the small size', () => {
    const svg = render({ id: 'mochi', size: SMALL_PX - 4, state: 'idle' });
    expect(count(svg, '.ca-dim')).toBe(0);
    expect(bodyDots(svg).filter((d) => d.getAttribute('opacity') === '0.5')).toHaveLength(cellCount('mochi', '+'));
  });

  it('keeps only the exclamation mark among the overlays', () => {
    expect(overlayDots(render({ id: 'mochi', size: 24, state: 'working' }))).toHaveLength(0);
    expect(overlayDots(render({ id: 'mochi', size: 24, state: 'sleeping' }))).toHaveLength(0);
    expect(overlayDots(render({ id: 'mochi', size: 24, state: 'needs_you' }))).toHaveLength(4);
  });
});

describe('Avatar motion', () => {
  it('blinks the eyes after a random wait while idle, for 120 ms', () => {
    vi.useFakeTimers();
    vi.spyOn(Math, 'random').mockReturnValue(0);
    const svg = render({ id: 'mochi', state: 'idle' });
    const resting = cellCount('mochi', '+');
    expect(count(svg, '.ca-dim')).toBe(resting);

    act(() => {
      vi.advanceTimersByTime(BLINK_MIN_MS);
    });
    expect(count(svg, '.ca-dim')).toBe(resting + cellCount('mochi', 'e'));

    act(() => {
      vi.advanceTimersByTime(BLINK_MS);
    });
    expect(count(svg, '.ca-dim')).toBe(resting);
  });

  it('does not blink while the state is not idle', () => {
    vi.useFakeTimers();
    vi.spyOn(Math, 'random').mockReturnValue(0);
    const svg = render({ id: 'mochi', state: 'working' });
    act(() => {
      vi.advanceTimersByTime(BLINK_MIN_MS + BLINK_MS);
    });
    expect(count(svg, '.ca-dim')).toBe(cellCount('mochi', '+'));
  });
});

describe('Avatar reduced motion', () => {
  beforeEach(() => stubReducedMotion(true));

  it('renders no motion class in any state', () => {
    for (const state of STATES) {
      const svg = render({ id: 'nib', state });
      expect(svg.querySelectorAll('[class*="ca-move"]')).toHaveLength(0);
      expect(svg.children[0]!.getAttribute('class') ?? '').not.toContain('ca-move');
    }
  });

  it('keeps the working and sleeping overlays static', () => {
    expect(count(render({ id: 'bit', state: 'working' }), '.ca-ripple')).toBe(3);
    expect(count(render({ id: 'bit', state: 'sleeping' }), '.ca-z')).toBe(7);
  });

  it('does not blink', () => {
    vi.useFakeTimers();
    vi.spyOn(Math, 'random').mockReturnValue(0);
    const svg = render({ id: 'mochi', state: 'idle' });
    act(() => {
      vi.advanceTimersByTime(BLINK_MIN_MS * 3);
    });
    expect(count(svg, '.ca-dim')).toBe(cellCount('mochi', '+'));
  });
});

// @vitest-environment jsdom
import { act } from 'react';
import { createRoot, type Root } from 'react-dom/client';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { DotSpinner } from './DotSpinner';

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

let host: HTMLDivElement;
let root: Root;

function render(status: Parameters<typeof DotSpinner>[0]['status'], size?: 3 | 5): HTMLElement {
  act(() => {
    root.render(<DotSpinner status={status} size={size} />);
  });
  return host.querySelector('.pm-dots') as HTMLElement;
}

/** Levels as a 2D grid of strings, e.g. "on", "dim", "off", "sweep". */
function levels(el: HTMLElement, n: number): string[][] {
  const dots = Array.from(el.querySelectorAll<HTMLElement>('.pm-dots-dot'));
  const rows: string[][] = [];
  for (let r = 0; r < n; r++) {
    rows.push(
      dots.slice(r * n, r * n + n).map((d) => {
        const m = /pm-dots-(on|dim|off|sweep)\b/.exec(d.className);
        return m ? m[1] : '?';
      }),
    );
  }
  return rows;
}

beforeEach(() => {
  host = document.createElement('div');
  document.body.appendChild(host);
  root = createRoot(host);
});

afterEach(() => {
  act(() => root.unmount());
  host.remove();
});

describe('DotSpinner patterns', () => {
  it('draws an error X on the diagonals', () => {
    expect(levels(render('error'), 3)).toEqual([
      ['on', 'off', 'on'],
      ['off', 'on', 'off'],
      ['on', 'off', 'on'],
    ]);
  });

  it('draws the exited check mark, dimmed', () => {
    expect(levels(render('exited'), 3)).toEqual([
      ['off', 'off', 'dim'],
      ['dim', 'off', 'off'],
      ['off', 'dim', 'off'],
    ]);
  });

  it('draws idle corners only, dimmed', () => {
    const grid = levels(render('idle'), 3);
    expect(grid).toEqual([
      ['dim', 'off', 'dim'],
      ['off', 'off', 'off'],
      ['dim', 'off', 'dim'],
    ]);
  });

  it('draws the waiting_input plus with a full-brightness centre', () => {
    const grid = levels(render('waiting_input'), 3);
    expect(grid).toEqual([
      ['off', 'dim', 'off'],
      ['dim', 'on', 'dim'],
      ['off', 'dim', 'off'],
    ]);
  });

  it('runs a single perimeter sweep and leaves the centre off', () => {
    const grid = levels(render('running'), 3);
    expect(grid[1][1]).toBe('off');
    const sweeps = grid.flat().filter((v) => v === 'sweep');
    expect(sweeps).toHaveLength(8);
    expect(grid.flat().filter((v) => v === 'on')).toHaveLength(0);
  });

  it('gives each sweep dot a distinct animation delay within one revolution', () => {
    const el = render('running');
    const delays = Array.from(el.querySelectorAll<HTMLElement>('.pm-dots-sweep')).map((d) => d.style.animationDelay);
    expect(new Set(delays).size).toBe(8);
    expect(delays[0]).toBe('0s');
  });

  it('supports the 5x5 size', () => {
    const el = render('error', 5);
    expect(el.querySelectorAll('.pm-dots-dot')).toHaveLength(25);
    expect(el.classList.contains('pm-dots-5')).toBe(true);
    expect(levels(el, 5)[2][2]).toBe('on');
  });
});

describe('DotSpinner accessibility', () => {
  it.each([
    ['running', 'Working'],
    ['waiting_input', 'Needs you'],
    ['idle', 'Idle'],
    ['error', 'Error'],
    ['exited', 'Exited'],
  ] as const)('labels %s as "%s"', (status, label) => {
    const el = render(status);
    expect(el.getAttribute('role')).toBe('img');
    expect(el.getAttribute('aria-label')).toBe(label);
  });
});

// Dot-matrix state glyph for the cockpit (STYLE.md v3, "dot matrix").
// ONE animated pattern (running: a clockwise sweep round the perimeter); every other state is static.
// Dots are circles drawn in currentColor. Off dots sit at low opacity, on dots at full opacity.
// Under prefers-reduced-motion the running sweep is replaced by a static perimeter ring (see dot-spinner.css).
import type { CSSProperties } from 'react';

type DotSize = 3 | 5;
type DotLevel = 'off' | 'dim' | 'on' | 'sweep';

const STATE_LABEL: Record<'running' | 'waiting_input' | 'idle' | 'error' | 'exited', string> = {
  running: 'Working',
  waiting_input: 'Needs you',
  idle: 'Idle',
  error: 'Error',
  exited: 'Exited',
};

/** Tick mark (check), [row, col] pairs, copied from the mockup's DotMatrix. */
const CHECK: Record<DotSize, [number, number][]> = {
  3: [[1, 0], [2, 1], [0, 2]],
  5: [[3, 0], [4, 1], [3, 2], [2, 3], [1, 4]],
};

/** Perimeter cells in clockwise order starting at the top-left corner. */
function perimeterOrder(n: number): [number, number][] {
  const last = n - 1;
  const out: [number, number][] = [];
  for (let c = 0; c <= last; c++) out.push([0, c]);
  for (let r = 1; r <= last; r++) out.push([r, last]);
  for (let c = last - 1; c >= 0; c--) out.push([last, c]);
  for (let r = last - 1; r >= 1; r--) out.push([r, 0]);
  return out;
}

function cellLevel(status: DotSpinnerProps['status'], n: number, r: number, c: number, perimeter: Map<string, number>): { level: DotLevel; order: number } {
  const m = (n - 1) / 2;
  const last = n - 1;
  switch (status) {
    case 'running': {
      const k = perimeter.get(`${r},${c}`);
      return k === undefined ? { level: 'off', order: 0 } : { level: 'sweep', order: k / perimeter.size };
    }
    case 'waiting_input': {
      // Plus shape: arms at 60%, centre at full brightness.
      if (r === m && c === m) return { level: 'on', order: 0 };
      if (r === m || c === m) return { level: 'dim', order: 0 };
      return { level: 'off', order: 0 };
    }
    case 'idle':
      return { level: (r === 0 || r === last) && (c === 0 || c === last) ? 'dim' : 'off', order: 0 };
    case 'error':
      return { level: r === c || r + c === last ? 'on' : 'off', order: 0 };
    case 'exited':
      return { level: CHECK[n as DotSize].some(([a, b]) => a === r && b === c) ? 'dim' : 'off', order: 0 };
  }
}

export interface DotSpinnerProps {
  status: 'running' | 'waiting_input' | 'idle' | 'error' | 'exited';
  size?: 3 | 5;
  className?: string;
}

export function DotSpinner({ status, size = 3, className }: DotSpinnerProps): JSX.Element {
  const n = size;
  const px = n === 3 ? 12 : 36;
  // Gap is about 40% of the dot size (same formula as the mockup).
  const gap = Math.max(1, Math.round((px / (n + (n - 1) * 0.4)) * 0.4));
  const dot = (px - gap * (n - 1)) / n;
  const perimeter = new Map<string, number>();
  if (status === 'running') perimeterOrder(n).forEach(([r, c], k) => perimeter.set(`${r},${c}`, k));

  const cells = [];
  for (let r = 0; r < n; r++) {
    for (let c = 0; c < n; c++) {
      const { level, order } = cellLevel(status, n, r, c, perimeter);
      const style: CSSProperties | undefined =
        level === 'sweep' ? { animationDelay: `${-order}s` } : undefined;
      cells.push(<i key={r * n + c} className={`pm-dots-dot pm-dots-${level}`} style={style} />);
    }
  }

  const rootClass = ['pm-dots', `pm-dots-${n}`, className].filter(Boolean).join(' ');
  return (
    <span
      className={rootClass}
      role="img"
      aria-label={STATE_LABEL[status]}
      style={{
        width: px,
        height: px,
        gridTemplateColumns: `repeat(${n}, ${dot}px)`,
        gridAutoRows: `${dot}px`,
        gap,
      }}
    >
      {cells}
    </span>
  );
}

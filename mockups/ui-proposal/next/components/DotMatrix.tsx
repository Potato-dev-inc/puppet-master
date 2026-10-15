import type { CSSProperties } from "react";
import type { AState } from "@/lib/mock";

type Size = 3 | 5;

const CHECK: Record<Size, [number, number][]> = {
  3: [[1, 0], [2, 1], [0, 2]],
  5: [[3, 0], [4, 1], [3, 2], [2, 3], [1, 4]],
};

function cell(state: AState, n: Size, r: number, c: number): { on: boolean; delay: number; mode: string } {
  const m = (n - 1) / 2;
  const last = n - 1;
  switch (state) {
    case "needs":
      return { on: r === m && c === m, delay: 0, mode: "pulse" };
    case "running":
      return { on: false, delay: (r + c) * 0.12, mode: "sweep" };
    case "tool":
      return { on: false, delay: c * 0.16, mode: "sweep" };
    case "writing":
      return { on: false, delay: (r * n + c) * 0.07, mode: "sweep" };
    case "thinking": {
      const d = Math.max(Math.abs(r - m), Math.abs(c - m));
      const a = (Math.atan2(r - m, c - m) + Math.PI) / (2 * Math.PI);
      return { on: false, delay: d * 0.5 + a * 0.5, mode: "slow" };
    }
    case "idle":
      return { on: (r === 0 || r === last) && (c === 0 || c === last), delay: 0, mode: "idle" };
    case "error":
      return { on: r === c || r + c === last, delay: 0, mode: "err" };
    case "done":
      return { on: CHECK[n].some(([a, b]) => a === r && b === c), delay: 0, mode: "done" };
  }
}

export function DotMatrix({ state, size = 3, px = 12 }: { state: AState; size?: Size; px?: number }) {
  const gap = Math.max(1, Math.round((px / (size + (size - 1) * 0.4)) * 0.4));
  const dot = (px - gap * (size - 1)) / size;
  const cells = [];
  for (let r = 0; r < size; r++)
    for (let c = 0; c < size; c++) {
      const x = cell(state, size, r, c);
      cells.push(
        <i key={r * size + c} className={`dm-${x.mode} ${x.on ? "on" : ""}`} style={{ "--d": `${x.delay}s` } as CSSProperties} />
      );
    }
  return (
    <span className="dotmatrix" role="img" aria-label={state}
      style={{ width: px, height: px, gridTemplateColumns: `repeat(${size}, ${dot}px)`, gridAutoRows: `${dot}px`, gap }}>
      {cells}
    </span>
  );
}

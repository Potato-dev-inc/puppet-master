import { PaneState, STATE_META } from "@/lib/mock";

export function StateChip({ state, since }: { state: PaneState; since?: string }) {
  const m = STATE_META[state];
  const live = state === "needs" || state === "thinking" || state === "running" || state === "tool";
  return (
    <span className={`chip ${live ? "pulse" : ""}`} style={{ color: m.color }}>
      <i />
      <span style={{ color: "var(--ink)" }}>{m.label}</span>
      {since && <span style={{ color: "var(--ink-3)", fontWeight: 600 }}>{since}</span>}
    </span>
  );
}

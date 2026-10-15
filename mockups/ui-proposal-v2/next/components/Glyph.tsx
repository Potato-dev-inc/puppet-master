import type { AState } from "@/lib/mock";

export function Glyph({ s }: { s: AState }) {
  const c = "glyph";
  if (s === "needs")
    return <span className={`${c} g-needs`} aria-hidden><i className="ring" /><i className="dot" /></span>;
  if (s === "running" || s === "tool")
    return (
      <svg className={`${c} g-spin`} viewBox="0 0 12 12" aria-hidden>
        <path d="M6 1.5a4.5 4.5 0 1 1-4.5 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    );
  if (s === "thinking")
    return <span className={`${c} g-think`} aria-hidden><i /><i /><i /></span>;
  if (s === "error")
    return (
      <svg className={`${c} g-err`} viewBox="0 0 12 12" aria-hidden>
        <rect x=".75" y=".75" width="10.5" height="10.5" rx="3" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <path d="M6 3.4v3.2M6 8.4v.1" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    );
  if (s === "done")
    return (
      <svg className={`${c} g-done`} viewBox="0 0 12 12" aria-hidden>
        <path d="M2.2 6.4l2.5 2.4 5-5.4" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
      </svg>
    );
  return <span className={`${c} g-idle`} aria-hidden />;
}

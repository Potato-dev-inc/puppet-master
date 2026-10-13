export type PaneState = "running" | "thinking" | "tool" | "needs" | "idle" | "error";

export const STATE_META: Record<PaneState, { label: string; color: string }> = {
  needs: { label: "Needs you", color: "var(--flag-yellow)" },
  error: { label: "Error", color: "var(--flag-red)" },
  thinking: { label: "Thinking", color: "var(--phase-think)" },
  tool: { label: "Tool", color: "var(--phase-tool)" },
  running: { label: "Running", color: "var(--phase-write)" },
  idle: { label: "Idle", color: "var(--ink-3)" },
};

export type Pane = {
  id: string; agent: string; project: string; state: PaneState; since: string;
  cols: number; rows: number; lines: string[]; prompt?: { question: string; answers: string[] };
};

export const PANES: Pane[] = [
  { id: "claude-a41f", agent: "Claude Code", project: "puppet-master", state: "needs", since: "2m", cols: 120, rows: 36,
    lines: ["● Edit(packages/app/src/tray.rs)", "  ⎿ Updated 14 lines", "Run `cargo test -p app tray`?"],
    prompt: { question: "Run cargo test -p app tray?", answers: ["Yes", "Yes, always", "No"] } },
  { id: "codex-77b2", agent: "Codex", project: "billing-api", state: "needs", since: "40s", cols: 110, rows: 32,
    lines: ["Proposed: rm -rf ./migrations/tmp", "Allow command outside workspace?"],
    prompt: { question: "Allow rm -rf ./migrations/tmp?", answers: ["Allow once", "Deny"] } },
  { id: "kilo-19cd", agent: "Kilo", project: "docs-site", state: "error", since: "5m", cols: 100, rows: 30,
    lines: ["error: provider 429 rate limited", "retry 3/3 failed", "process exited with code 1"] },
  { id: "claude-c0de", agent: "Claude Code", project: "mobile-pwa", state: "thinking", since: "12s", cols: 120, rows: 36,
    lines: ["✻ Considering the safe-area insets…", "  (esc to interrupt)"] },
  { id: "opencode-5e5e", agent: "OpenCode", project: "infra", state: "tool", since: "8s", cols: 100, rows: 30,
    lines: ["$ terraform plan -out=tf.plan", "Refreshing state... [aws_s3_bucket.logs]"] },
  { id: "codex-02aa", agent: "Codex", project: "puppet-master", state: "running", since: "1m", cols: 110, rows: 32,
    lines: ["Writing tests/pane_state.rs", "+ fn classifies_needs_you() {"] },
  { id: "shell-9f00", agent: "PowerShell", project: "scratch", state: "idle", since: "14m", cols: 90, rows: 24,
    lines: ["PS C:\\scratch> _"] },
];

export const WARM_AGENTS = [
  { name: "Claude Code", ready: true }, { name: "Codex", ready: true },
  { name: "OpenCode", ready: false }, { name: "Kilo", ready: true }, { name: "PowerShell", ready: true },
];

export const TERMINAL_FEED: { t: string; s: string }[] = [
  { t: "dim", s: "~/work/puppet-master  claude --resume" },
  { t: "txt", s: "● I will wire the tray Quit item through a confirm step." },
  { t: "tool", s: "● Read(packages/app/src-tauri/src/tray.rs)" },
  { t: "dim", s: "  ⎿ Read 412 lines" },
  { t: "tool", s: "● Edit(packages/app/src-tauri/src/tray.rs)" },
  { t: "dim", s: "  ⎿ Updated with 14 additions and 3 removals" },
  { t: "add", s: "+     if panes.running() > 0 { confirm_quit(app); return; }" },
  { t: "del", s: "-     app.exit(0);" },
  { t: "txt", s: "● Now running the tray tests." },
  { t: "need", s: "? Run `cargo test -p app tray`?  [y] yes  [a] always  [n] no" },
];

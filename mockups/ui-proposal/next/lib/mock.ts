export type AState = "needs" | "writing" | "running" | "tool" | "thinking" | "idle" | "error" | "done";

export const STATE_TEXT: Record<AState, string> = {
  needs: "Needs you", running: "Running", writing: "Writing", tool: "Running tool", thinking: "Thinking", idle: "Idle", error: "Error", done: "Done",
};

export type TLine = { k: "prompt" | "bullet" | "out" | "dim" | "add" | "del" | "user"; s: string };

export type Agent = {
  id: string; name: string; project: string; state: AState; sec: number;
  cols: number; rows: number; phone?: boolean; spawning?: boolean; lines: TLine[];
  question?: { text: string; answers: string[] };
};

export const AGENTS: Agent[] = [
  { id: "a1", name: "Claude Code", project: "puppet-master", state: "needs", sec: 120, cols: 120, rows: 36,
    question: { text: "Run cargo test -p app tray?", answers: ["Allow", "Always allow", "Deny"] },
    lines: [
      { k: "prompt", s: "puppet-master % claude --resume" },
      { k: "bullet", s: "I will route the tray Quit item through a confirm step." },
      { k: "bullet", s: "Read(src-tauri/src/tray.rs)" }, { k: "dim", s: "  Read 412 lines" },
      { k: "bullet", s: "Edit(src-tauri/src/tray.rs)" }, { k: "dim", s: "  Updated with 14 additions and 3 removals" },
      { k: "add", s: "+  if panes.running() > 0 { confirm_quit(app); return; }" },
      { k: "del", s: "-  app.exit(0);" },
      { k: "bullet", s: "Now running the tray tests." },
    ] },
  { id: "a2", name: "Codex", project: "billing-api", state: "needs", sec: 40, cols: 110, rows: 32, phone: true,
    question: { text: "Allow rm -rf ./migrations/tmp?", answers: ["Allow once", "Deny"] },
    lines: [
      { k: "prompt", s: "billing-api % codex" },
      { k: "bullet", s: "Cleaning stale migration scratch files." },
      { k: "out", s: "Proposed: rm -rf ./migrations/tmp" },
      { k: "dim", s: "Command runs outside the workspace sandbox." },
    ] },
  { id: "a3", name: "Claude Code", project: "mobile-pwa", state: "thinking", sec: 12, cols: 120, rows: 36,
    lines: [{ k: "prompt", s: "mobile-pwa % claude" }, { k: "bullet", s: "Checking safe-area insets on the composer." }, { k: "dim", s: "  esc to interrupt" }] },
  { id: "a4", name: "OpenCode", project: "infra", state: "tool", sec: 8, cols: 100, rows: 30,
    lines: [{ k: "prompt", s: "infra % opencode" }, { k: "out", s: "$ terraform plan -out=tf.plan" }, { k: "dim", s: "Refreshing state... aws_s3_bucket.logs" }] },
  { id: "a5", name: "Codex", project: "puppet-master", state: "writing", sec: 60, cols: 110, rows: 32,
    lines: [{ k: "prompt", s: "puppet-master % codex" }, { k: "bullet", s: "Writing tests/pane_state.rs" }, { k: "add", s: "+  fn classifies_needs_you() {" }, { k: "del", s: "-  // TODO" }] },
  { id: "a6", name: "Kilo", project: "docs-site", state: "error", sec: 300, cols: 100, rows: 30,
    lines: [{ k: "prompt", s: "docs-site % kilo" }, { k: "out", s: "provider returned 429, rate limited" }, { k: "dim", s: "retry 3 of 3 failed" }, { k: "out", s: "process exited with code 1" }] },
  { id: "a7", name: "PowerShell", project: "scratch", state: "idle", sec: 840, cols: 90, rows: 24,
    lines: [{ k: "prompt", s: "scratch %" }] },
  { id: "a8", name: "Claude Code", project: "docs-site", state: "done", sec: 1320, cols: 120, rows: 36,
    lines: [{ k: "bullet", s: "Updated the README install section." }, { k: "dim", s: "  3 files changed" }] },
];

export const WARM = [
  { name: "Claude Code", ready: true }, { name: "Codex", ready: true }, { name: "OpenCode", ready: false },
  { name: "Kilo", ready: true }, { name: "PowerShell", ready: true },
];

export function group(a: Agent): "Needs you" | "Working" | "Idle" {
  if (a.state === "needs" || a.state === "error") return "Needs you";
  if (a.state === "idle" || a.state === "done") return "Idle";
  return "Working";
}

export function fmt(sec: number): string {
  const m = Math.floor(sec / 60), s = sec % 60;
  if (m === 0) return `${s}s`;
  const h = Math.floor(m / 60);
  return h > 0 ? `${h}h ${m % 60}m` : `${m}m`;
}

export const SPAWN_LOG = ["Claiming warm standby…", "Attaching PTY 120×36", "Loading TOML profile kilo", "Ready."];

export const MODELS = ["Claude Code · Opus 5.5", "Claude Code · Sonnet 5.5", "Codex · GPT-5", "Kilo · Auto"];

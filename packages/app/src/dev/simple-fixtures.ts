// Example data for the Simple mode scenarios (dev harness only): panes that work on a copy, the page
// the agent made, a conversation, saved versions, and the answers of the Simple commands.
import type { PaneInfo } from '../lib/tauri-types';
import type { PanePrompt } from '../lib/cockpit-api';
import cases from '../components/simple/folder-cases.json';
import { folderVerdict, type KnownDirs } from '../components/simple/folder-guard';

export interface SimplePaneFixture {
  id: string;
  status: PaneInfo['status'];
  /** The project's own folder (the copy lives inside it). */
  repo: string;
  prompt?: PanePrompt | null;
  /** The page in the copy; `null` = none yet. */
  html?: string | null;
  filesChanged?: number;
  /** `pane_run_info` error code, for the problem card. */
  errorCode?: string;
  /** The run failed while the agent is idle (`PROMPT_NOT_SUBMITTED`): a message that never reached it. */
  runFailedCode?: string;
  /** No transcript lines at all. */
  emptyConversation?: boolean;
}

export interface SimpleScenario {
  panes: SimplePaneFixture[];
  /** What the folder picker returns (the "use a folder I already have" step). */
  pickedFolder?: { path: string; name: string };
  /** No agent is ready on this computer. */
  noAgent?: boolean;
}

const now = Date.now();
const MIN = 60_000;

export const QUIZ_REPO = 'C:\\Users\\Ana\\Documents\\PM\\Plants quiz';
export const TIMER_REPO = 'C:\\Users\\Ana\\Documents\\PM\\Class timer';
export const ORDERS_REPO = 'C:\\Users\\Ana\\Documents\\PM\\Order form';

export const copyPath = (repo: string) => `${repo}\\.puppet-master\\worktrees\\a1b2`;

export function simplePaneInfo(p: SimplePaneFixture): PaneInfo {
  return {
    id: p.id,
    agent_type: 'claude',
    pid: 4000 + p.id.length,
    status: p.status,
    created_at: now - 12 * MIN,
    last_output_at: now - 5_000,
    cwd: copyPath(p.repo),
    cols: 120,
    rows: 32,
  };
}

/** A real, working page: what a template run leaves behind. Self-contained, no network. */
export const QUIZ_HTML = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>Plants quiz</title>
<style>
  body{font:18px/1.5 system-ui,sans-serif;margin:0;background:#f4f4f5;color:#18181b}
  main{max-width:560px;margin:0 auto;padding:32px 24px}
  h1{font-size:28px;margin:0 0 4px}
  p.sub{margin:0 0 24px;color:#52525b}
  .q{background:#fff;border:1px solid #e4e4e7;border-radius:14px;padding:20px}
  .q h2{font-size:20px;margin:0 0 14px}
  label{display:block;padding:12px 14px;margin:8px 0;border:1px solid #d4d4d8;border-radius:10px;cursor:pointer}
  label:hover{background:#f4f4f5}
  button{margin-top:16px;padding:12px 22px;font-size:17px;border:0;border-radius:10px;background:#18181b;color:#fff;cursor:pointer}
  #score{display:none;font-size:22px;font-weight:700}
</style></head><body><main>
<h1>Plants quiz</h1><p class="sub">Question 1 of 10</p>
<div class="q" id="q"><h2>Which part of the plant makes food?</h2>
<label><input type="radio" name="a"> Root</label>
<label><input type="radio" name="a" checked> Leaf</label>
<label><input type="radio" name="a"> Stem</label>
<button id="next">Next question</button></div>
<p id="score">Your score: 10 of 10</p>
</main><script>
document.getElementById('next').addEventListener('click',function(){document.getElementById('score').style.display='block'});
</script></body></html>`;

export const QUIZ_TASK =
  'Make a quiz with 10 questions about plants for my students. It should look bright and fun. Make it as ONE file called tool.html. Make it as one file. Put everything in that file. It must work without the internet. Do not use other programs. Do not delete or move any files. Keep the words on the page short and simple. At the end, say what you made in 3 short sentences.';

export const SIMPLE_TRANSCRIPT = {
  chunks: [
    { stream: 'user_task', text: QUIZ_TASK },
    { stream: 'delivery', text: 'Sent. The agent started.' },
    { stream: 'assistant_final', text: '● Write(C:\\Users\\Ana\\Documents\\PM\\Plants quiz\\tool.html)\n  ⎿  Wrote 148 lines' },
    { stream: 'assistant_final', text: '● Read(tool.html)' },
    { stream: 'assistant_final', text: 'I made a quiz with 10 questions about plants. It shows the score at the end. It works without the internet.' },
    { stream: 'user_steer', text: 'Make the buttons bigger.' },
    { stream: 'assistant_final', text: '● Edit(tool.html)' },
    { stream: 'assistant_final', text: 'The buttons are bigger now.' },
  ],
};

const ck = (id: string, turn: number, kind: string, label: string, ago: number, files: number) => ({
  id,
  commit: `c${id}`,
  paneId: 'x',
  turn,
  kind,
  label,
  note: '',
  atMs: now - ago,
  files,
  added: files * 20,
  deleted: 0,
});

export const SIMPLE_CHECKPOINTS = {
  state: 'ok',
  message: '',
  items: [
    ck('6', 2, 'after', 'After turn 2', 2 * MIN, 1),
    ck('5', 2, 'before', 'Before turn 2', 6 * MIN, 0),
    ck('4', 1, 'after', 'After turn 1', 11 * MIN, 1),
    ck('1', 1, 'before', 'Before turn 1', 13 * MIN, 0),
  ],
};

export const SIMPLE_SUMMARIES = [
  { paneId: 'x', agent: 'Claude Code', turn: 1, startedAtMs: now - 13 * MIN, endedAtMs: now - 11 * MIN, durationMs: 2 * MIN, files: { count: 1, added: 148, deleted: 0, paths: ['tool.html'] }, approvals: [], end: { kind: 'idle' }, sentence: '' },
  { paneId: 'x', agent: 'Claude Code', turn: 2, startedAtMs: now - 6 * MIN, endedAtMs: now - 2 * MIN, durationMs: 4 * MIN, files: { count: 1, added: 12, deleted: 4, paths: ['tool.html'] }, approvals: [], end: { kind: 'idle' }, sentence: '' },
];

export const SIMPLE_DIFF = {
  branch: 'pm/a1b2',
  base: 'main',
  changes: {
    files: [
      { path: 'tool.html', status: 'added', additions: 148, deletions: 0, binary: false },
      { path: 'images/leaf.png', status: 'added', additions: 0, deletions: 0, binary: true },
    ],
    files_changed: 2,
    additions: 148,
    deletions: 0,
    truncated: false,
  },
  patch: '',
  patch_truncated: false,
};

const KNOWN = cases.known as KnownDirs;

export function simpleMock(
  sc: SimpleScenario | undefined,
  cmd: string,
  a: Record<string, unknown>,
): { handled: false } | { handled: true; value: unknown } {
  if (!sc) return { handled: false };
  const pane = sc.panes.find((p) => p.id === a.paneId);
  const ok = (value: unknown) => ({ handled: true as const, value });
  switch (cmd) {
    case 'plain_check_folder': {
      // The same cases the Rust guard is tested with.
      const verdict = folderVerdict(String(a.path ?? ''), KNOWN);
      return ok(verdict === 'ok' ? { ok: true } : { ok: false, code: 'FOLDER_TOO_BROAD', message: verdict });
    }
    case 'plain_start':
      return ok({ pane_id: 'pane-new', handle: 'h1', folder: QUIZ_REPO });
    case 'project_pick_folder':
      return ok(sc.pickedFolder ?? null);
    case 'plain_send':
      return ok({ status: 'running', pane_id: 'pane-quiz' });
    case 'plain_preview':
      return ok(pane?.html ? { name: 'tool.html', html: pane.html } : null);
    case 'plain_open_preview':
      return ok(null);
    case 'agents_list':
      return sc.noAgent ? ok({ agents: [], dispatchable_ids: [] }) : { handled: false };
    case 'pane_worktree':
      return pane
        ? ok({
            handle: 'a1b2',
            branch: 'pm/a1b2',
            base: 'main',
            repo: pane.repo,
            path: copyPath(pane.repo),
            exists: true,
            state: pane.status === 'running' ? 'active' : 'unreviewed',
            summary: { files_changed: pane.filesChanged ?? 2, additions: 148, deletions: 0 },
          })
        : ok(null);
    case 'pane_prompt':
      return pane ? ok(pane.prompt ?? null) : { handled: false };
    case 'pane_run_info':
      return pane ? ok({ handle: 'run_1', status: pane.status === 'error' || pane.runFailedCode ? 'failed' : 'running', receipt: null, error: pane.errorCode || pane.runFailedCode ? { code: pane.errorCode ?? pane.runFailedCode } : null }) : { handled: false };
    case 'pane_transcript':
      return pane ? ok(pane.emptyConversation ? { chunks: [] } : SIMPLE_TRANSCRIPT) : { handled: false };
    case 'checkpoints_list':
      return pane ? ok(SIMPLE_CHECKPOINTS) : { handled: false };
    case 'turn_summaries':
      return pane ? ok(SIMPLE_SUMMARIES) : { handled: false };
    case 'worktree_diff':
      return sc.panes.length ? ok(SIMPLE_DIFF) : { handled: false };
    default:
      return { handled: false };
  }
}

// ---- prompts (fixtures, not real agent output)

export const ASK_PROMPT: PanePrompt = {
  promptId: 'simple-1',
  kind: 'command_approval',
  text: 'Do you want to run this command?\n  node --check tool.html',
  summary: 'Run node --check tool.html',
  choices: [
    { id: 'yes', label: 'Yes' },
    { id: 'yes-always', label: 'Yes, and do not ask again', broad: true },
    { id: 'no', label: 'No, tell Claude what to do differently' },
  ],
  risk: { level: 'low', headline: 'Run tests', reasons: ["It runs your project's tests and does not change anything else."] },
};

export const ASK_NETWORK_PROMPT: PanePrompt = {
  ...ASK_PROMPT,
  promptId: 'simple-2',
  summary: 'Run curl https://example.com/leaf.png',
  text: 'Do you want to run this command?\n  curl https://example.com/leaf.png',
  risk: { level: 'medium', headline: 'Connect to another computer', reasons: ['It talks to another computer over the network.'] },
};

export const BLOCKED_PROMPT: PanePrompt = {
  promptId: 'simple-3',
  kind: 'command_approval',
  text: 'Run this command?\n  rm -rf ../old-site',
  summary: 'Run command: rm -rf ../old-site',
  choices: [
    { id: 'allow_once', label: 'Allow once' },
    { id: 'allow_all', label: 'Always allow', broad: true },
    { id: 'deny', label: 'Deny' },
  ],
  risk: { level: 'high', headline: 'Delete a folder outside this project', reasons: ['It deletes ../old-site and everything in it.'] },
};

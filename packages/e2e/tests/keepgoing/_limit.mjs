// A Claude Code that runs out of usage, and the Codex that could take over: both are the fake agent on the app's PATH
// under the real programs' names, so the real limit detector and the real hand-over run. Not a test.
import { gitProject, installFakeProgram } from '../night/_area.mjs';

export const CLAUDE_AT_LIMIT = `agent = "claude"
boot_ms = 100
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "say", text = "Working on it." },
  { do = "limit", text = "⎿  You've hit your session limit · resets 5pm", reset = "5pm" },
]
`;

export const CODEX_CARRIES_ON = `agent = "codex"
boot_ms = 100
[[turn]]
steps = [ { do = "think", ms = 300 }, { do = "say", text = "Carrying on from the brief." } ]
`;

/**
 * Start a fake Claude Code in a git project and let it hit its usage limit. Returns { pane, dir, undo }; call `undo()` in a
 * `finally` to take the fake programs off the PATH again.
 */
export async function claudeHitsItsLimit(t, { project = 'limit-app', codex = false } = {}) {
  const undos = [installFakeProgram(t, 'claude')];
  if (codex) undos.push(installFakeProgram(t, 'codex'));
  const undo = () => undos.forEach((u) => u());
  try {
    t.fake.script(CLAUDE_AT_LIMIT);
    const dir = gitProject(t, project);
    const pane = await t.spawnPane('claude', { cwd: dir, waitFor: /❯/, idle: false });
    // the fake paints its composer a moment before it takes keys
    await t.expect.eventually(async () => t.fake.events().some((e) => e.ev === 'ready'), { message: 'the fake Claude to be ready' });
    await t.sleep(1500);
    await pane.write('please go');
    await t.waitFor('exact=Dismiss', { timeout: 30000 });
    // Leave no limit card behind for the next test: dismiss it when this test is over (the card is in the Inbox for good).
    return { pane, dir, undo: () => { undo(); } };
  } catch (e) {
    undo();
    throw e;
  }
}

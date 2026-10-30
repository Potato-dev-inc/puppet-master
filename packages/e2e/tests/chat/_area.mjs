// Helpers shared by the chat, inbox and search tests (files that start with "_" are never run as tests).
import fs from 'node:fs';
import path from 'node:path';
import { fakeTuiExe } from '../../src/paths.mjs';
import { openChat as baseOpenChat, sendChat as baseSendChat, selectPane as baseSelectPane } from '../_helpers.mjs';

/**
 * After a few minutes without the window in front, the app shows "While you were away" over the whole cockpit the next time
 * the page loads (the test window is never in front). It hides every button below it, so each step that starts a
 * screen puts it away first with "Got it", the way a person would.
 */
export async function dismissAway(t) {
  if (await t.page.has('css=.ck-away')) {
    await t.click('exact=Got it');
    await t.waitGone('css=.ck-away');
  }
}

export async function selectPane(t, which = 0) {
  await dismissAway(t);
  return baseSelectPane(t, which);
}
export async function openChat(t, n = 0) {
  await dismissAway(t);
  return baseOpenChat(t, n);
}
export async function sendChat(t, text) {
  await dismissAway(t);
  return baseSendChat(t, text);
}

/** The text of the first fake agent definition ("nimbus"), changed into another agent that this area installs itself. */
function nimbusFixture() {
  return fs.readFileSync(new URL('../../fixtures/agents/nimbus.toml', import.meta.url), 'utf8');
}

/**
 * Install one more fake agent while the app runs: the nimbus screen with a different id, `idle_after_ms` or a
 * `[session_chat]` table. It writes the definition into the app's agents folder, gives the fake program the name the
 * definition asks for, reloads the definitions and approves it. Safe to call again (it does nothing the second time).
 * `extra` is appended to the definition (TOML tables such as `[session_chat]`).
 */
export async function installAgent(t, { id, idleMs = 1500, extra = '' }) {
  const app = t.app;
  const file = `${id}.toml`;
  const target = path.join(app.dataDir, 'agents', file);
  if (fs.existsSync(target)) return;
  let text = nimbusFixture()
    .replace('id = "nimbus"', `id = "${id}"`)
    .replace('label = "Nimbus"', `label = "${id}"`)
    .replace('names = ["nimbus"]', `names = ["${id}"]`)
    .replace('idle_after_ms = 1500', `idle_after_ms = ${idleMs}`);
  text += `\n${extra}\n`;
  fs.writeFileSync(target, text);
  fs.copyFileSync(fakeTuiExe(), path.join(app.dirs.bin, `${id}.exe`));
  await app.bridge.post('/agents/definitions/reload', {});
  const list = (await app.bridge.get('/agents/definitions')).body || {};
  const entries = Array.isArray(list) ? list : list.agents || list.definitions || list.entries || [];
  const e = entries.find((x) => (x.id || x.agent?.id) === id || String(x.file || '').endsWith(file));
  if (!e) throw new Error(`the agent ${id} was not loaded: ${JSON.stringify(list).slice(0, 500)}`);
  if (!(e.approved === true || e.state === 'approved' || e.status === 'approved' || e.status === 'active')) {
    const a = await app.bridge.post('/agents/definitions/approve', { file: e.file || file, fingerprint: e.fingerprint || e.sha || e.hash });
    if (a.status !== 200) throw new Error(`approving ${id} failed: ${a.status} ${JSON.stringify(a.body).slice(0, 300)}`);
  }
}

/** The agent whose idle window is the longest the app allows (5 s): a pane whose screen holds still still reads "running". */
export const installSlowIdleAgent = (t) => installAgent(t, { id: 'nimbus5', idleMs: 5000 });

/** An agent whose Chat tab reads a conversation file in `<project>/.scribe/chat.jsonl` (rich chat: markdown, tool rows). */
export const SCRIBE_TABLES = `
[session_chat]
format = "jsonl"
dir = "{cwd}/.scribe"
file_glob = "*.jsonl"
pick = "newest_since_start"
write_tools = ["write_file"]

[session_chat.map]
role = "/role"
text = "/content"
timestamp = "/ts"
model = "/model"
tool_name = "/call/name"
tool_input = "/call/args"
tool_id = "/call/id"
tool_result_for = "/result/for"
tool_output = "/result/output"
tool_error = ["/result/is_error", "/result/exit_code"]
usage_input = "/usage/in"
usage_output = "/usage/out"
turn_end = "/stop"

[session_chat.values]
user = ["user"]
assistant = ["assistant"]
tool = ["tool"]
turn_end = ["end_turn"]
`;
export const installScribe = (t) => installAgent(t, { id: 'scribe', extra: SCRIBE_TABLES });

/** Append lines to the conversation file of a scribe pane (they show in its Chat tab). */
export function writeScribeChat(pane, lines) {
  const dir = path.join(pane.cwd, '.scribe');
  fs.mkdirSync(dir, { recursive: true });
  const file = path.join(dir, 'chat.jsonl');
  fs.appendFileSync(file, lines.map((l) => (typeof l === 'string' ? l : JSON.stringify(l))).join('\n') + '\n');
  return file;
}

let ts = 1793178000;
/** One conversation line in the scribe format. */
export const said = {
  user: (content) => ({ ts: (ts += 2), role: 'user', content }),
  agent: (content, { final = true, model = 'scribe-1' } = {}) => ({ ts: (ts += 3), role: 'assistant', model, content, ...(final ? { stop: 'end_turn' } : {}) }),
  call: (id, name, args) => ({ ts: (ts += 1), role: 'tool', call: { id, name, args } }),
  result: (id, output, exit_code) => ({ ts: (ts += 1), role: 'tool', result: { for: id, output, ...(exit_code !== undefined ? { exit_code } : {}) } }),
};

/** A fake agent script: boot, then one turn (matching `on_prompt`, or any prompt) with these steps. `extra` goes at the top. */
export function turnScript(steps, { on_prompt, extra = '', approvals } = {}) {
  return `
boot_ms = 100
inline = true
${approvals ? `approvals = "${approvals}"` : ''}
${extra}
[[turn]]
${on_prompt ? `on_prompt = "${on_prompt}"` : ''}
steps = [
${steps.map((s) => `  ${s},`).join('\n')}
]
`;
}

/** A step that prints rows and then waits for a key (the question stays on screen, nothing moves). */
export const hold = (text) => `{ do = "hold", text = ${JSON.stringify(text)} }`;
export const think = (ms) => `{ do = "think", ms = ${ms} }`;
export const say = (text) => `{ do = "say", text = ${JSON.stringify(text)} }`;

/** The keys the fake agent has seen (its `key` events), as a list of names. */
export const keysSeen = (t) => t.fake.events().filter((e) => e.ev === 'key').map((e) => e.key);

/** The composer of the Chat tab. */
export const COMPOSER = 'aria=Message to the agent';

/** A fake agent: a prompt with "slow" takes 9 s; every other prompt gets "ok <last word>" quickly. */
export const SLOW_AND_ECHO = `
boot_ms = 100
inline = true
[[turn]]
on_prompt = "slow"
steps = [
  { do = "think", ms = 9000 },
  { do = "say", text = "slow job done" },
]
[[turn]]
steps = [
  { do = "think", ms = 200 },
  { do = "say", text = "ok {last_word}" },
]
`;

/** An agent whose status turns "waiting for you" on any screen that holds `please answer` (so the pane waits, with no approval card). */
export const installAsker = (t) => installAgent(t, { id: 'asker', extra: "\n[tui.classify]\nwaiting_regex = ['please answer']\n" });

/** An agent that asks before it creates hello.txt (a one-line [y/n] question) and reports what happened. */
export const APPROVE_FILE = `
boot_ms = 100
inline = true
[[turn]]
on_prompt = "make the file"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [{ do = "write_file", path = "hello.txt", content = "hi" }, { do = "say", text = "file written" }], on_deny = [{ do = "say", text = "nothing written" }] },
]
`;

/**
 * The app keeps its questions between tests (the reset closes panes and reloads the page, nothing more), so a test that
 * looks at the Inbox first dismisses what an earlier test left. Only questions created before this test started.
 */
export async function dropOldQuestions(t) {
  const list = (await t.bridge.get('/questions')).body.questions || [];
  for (const q of list) if (q.created_at_ms < t.startedAt) await t.bridge.post(`/questions/${encodeURIComponent(q.id)}/dismiss`, {});
}

/** An agent asks the person a question (the `ask_user` tool): returns the question id. Returns at once (it does not wait for the answer). */
export async function askUser(t, body) {
  await dropOldQuestions(t);
  const r = await t.bridge.post('/questions/ask', { wait_ms: 100, ...body });
  if (!r.ok) throw new Error(`ask_user failed: ${r.status} ${JSON.stringify(r.body).slice(0, 300)}`);
  return r.body.question_id;
}
/** What the agent would get back for a question (`resolved`, `choice_id` or `text`, `dismissed` ...). */
export async function questionResult(t, id) {
  return (await t.bridge.post('/questions/ask', { question_id: id, wait_ms: 100 })).body;
}
/** The questions waiting for the person. */
export async function pendingQuestions(t) {
  return (await t.bridge.get('/questions')).body.questions;
}
/** Open the Inbox from the sidebar. */
export async function openInbox(t) {
  await dismissAway(t);
  await dropOldQuestions(t);
  await t.click('exact=Inbox', { within: 'css=.ck-side' });
  await t.waitFor('css=.ck-inbox');
}

/** A scribe pane (its Chat reads a conversation file) in a folder of its own, with the Chat tab open. */
export async function scribeChat(t, name, lines = [], { cwd } = {}) {
  await installScribe(t);
  const pane = await t.spawnPane('scribe', { cwd: cwd || t.project(`scribe-${name}`) });
  await openChat(t);
  if (lines.length) writeScribeChat(pane, lines);
  return pane;
}

/** Click a target once it has stopped moving (a sheet or a menu that slides in is measured too early otherwise). */
export async function clickWhenStill(t, target, opts = {}) {
  let last = '';
  await t.expect.eventually(async () => {
    const r = (await t.page.find(target, opts)).rect;
    const key = r ? [r.x, r.y, r.w, r.h].map(Math.round).join(',') : '';
    const still = key !== '' && key === last;
    last = key;
    return still;
  }, { message: `${JSON.stringify(target)} did not settle`, interval: 200 });
  await t.click(target, opts);
}

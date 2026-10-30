// Shared kit of the "code tools" tests (What broke it, Branch from here, Collision Alert, Review desk, Scores,
// Point and Fix, Clip It, Package check). Every other area folder imports this file. Nothing in here touches the
// harness core: it only builds folders, scripts and waits on top of `t`.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fakeTuiExe } from '../../src/paths.mjs';
import { PANE_ROW } from '../_helpers.mjs';

export { PANE_ROW };

// ---------------------------------------------------------------- git

const cleanEnv = () => {
  const env = { ...process.env };
  for (const k of Object.keys(env)) if (k.startsWith('GIT_')) delete env[k];
  return env;
};

/** Run git inside a folder of the run (fixture setup only; never the app's own git). */
export function git(dir, ...args) {
  return execFileSync('git', ['-c', 'user.name=E2E', '-c', 'user.email=e2e@example.invalid', '-c', 'core.autocrlf=false', '-c', 'commit.gpgsign=false', '-c', 'init.defaultBranch=main', ...args], {
    cwd: dir, env: cleanEnv(), stdio: 'pipe', windowsHide: true,
  }).toString();
}

export function writeFiles(dir, files) {
  for (const [rel, content] of Object.entries(files)) {
    const p = path.join(dir, rel);
    fs.mkdirSync(path.dirname(p), { recursive: true });
    fs.writeFileSync(p, content);
  }
}

/**
 * A folder of this test only: the app keeps history (save points, scores) per folder and one app serves a whole run, so
 * two tests must never share a folder name. The test's own name is put in front of `name`.
 */
export function projectDir(t, name) {
  return t.project(`${t.meta.id.replace(/[^a-z0-9]+/g, '-')}-${name}`);
}

/** A new folder in the run's temp area with these files, as a git repository with one commit. */
export function gitProject(t, name, files = { 'README.md': 'demo\n' }) {
  const dir = projectDir(t, name);
  writeFiles(dir, files);
  git(dir, 'init', '-q');
  git(dir, 'add', '-A');
  git(dir, 'commit', '-q', '-m', 'start');
  return dir;
}

/** The test script the What broke it tests declare: red as soon as `bad.flag` exists. */
export const CHECK_MJS = `import fs from 'node:fs';
if (fs.existsSync('bad.flag')) { console.error('FAIL: bad.flag exists'); process.exit(1); }
console.log('ok');
`;

// ---------------------------------------------------------------- fake agent scripts (TOML)

const q = (s) => JSON.stringify(String(s)); // a TOML basic string is a JSON string for our texts

/**
 * A fake-agent script. Each turn answers a prompt that contains `prompt`:
 *   { prompt, think: 6000, writes: [[path, content]], say, then: 'raw toml step', hold: ms }
 * `think` first (long enough for the app to see the pane working), then the writes, then `say`.
 */
export function turnsScript(turns, { header = '' } = {}) {
  let out = `agent = "nimbus"\nboot_ms = 100\ninline = true\n${header}\n`;
  for (const turn of turns) {
    const steps = [];
    steps.push(`{ do = "think", ms = ${turn.think ?? 6000} }`);
    for (const [p, c] of turn.writes || []) steps.push(`{ do = "write_file", path = ${q(p)}, content = ${q(c)} }`);
    for (const s of turn.steps || []) steps.push(s);
    if (turn.hold) steps.push(`{ do = "think", ms = ${turn.hold} }`);
    if (turn.say !== null) steps.push(`{ do = "say", text = ${q(turn.say ?? `did ${turn.prompt}`)} }`);
    out += `[[turn]]\n${turn.prompt ? `on_prompt = ${q(turn.prompt)}\n` : ''}steps = [\n  ${steps.join(',\n  ')},\n]\n`;
  }
  return out;
}

// ---------------------------------------------------------------- waiting

export const waitIdle = (t, pane, timeout = 30000) => t.expect.eventually(async () => (await pane.info())?.status === 'idle', { timeout, message: `expected pane ${pane.id.slice(0, 8)} to be idle` });

/** Save points (History rows) of a pane, newest first. */
export async function savePoints(t, pane) {
  const r = await t.bridge.get(`/panes/${pane.id}/checkpoints`);
  return r.body?.items || [];
}

/** Wait for a save point of this kind ("before" / "after"). The app takes the "after" about 10 s after the "before". */
export const waitSavePoint = (t, pane, kind, timeout = 70000) => t.expect.eventually(async () => (await savePoints(t, pane)).find((c) => c.kind === kind), { timeout, message: async () => `expected a "${kind}" save point for pane ${pane.id.slice(0, 8)}. It has: ${JSON.stringify((await savePoints(t, pane)).map((c) => c.label))}; status ${(await pane.info())?.status}` });

/** Type a prompt into the pane the way a person at the terminal does and wait for the turn to start. */
export async function ask(t, pane, text) {
  await pane.write(text);
}

/**
 * One finished turn that has both of its save points. The fake agent's script must think for a few seconds before it
 * writes (see `turnsScript`), so the app's "before" save point is taken first; then the pane is idle and the "after"
 * save point is there. Starts a new pane in `cwd` for the turn: every pane has its own 10 s save point window.
 */
export async function savedTurn(t, cwd, prompt) {
  const pane = await t.spawnPane('nimbus', { cwd });
  await ask(t, pane, prompt);
  await waitSavePoint(t, pane, 'before');
  await waitIdle(t, pane);
  await waitSavePoint(t, pane, 'after');
  return pane;
}

// ---------------------------------------------------------------- sidebar and tabs

/** Select the pane whose sidebar title (its first message) contains this text. */
export async function selectByTitle(t, text) {
  await t.click(text, { within: 'css=.ck-side' });
}

export async function openTab(t, name) {
  await t.click(`exact=${name}`);
}

export async function openHistory(t) {
  await openTab(t, 'History');
  await t.waitFor('exact=What broke the tests?');
}

/** Open a sidebar page by its label (Scores, Review, Project ...). */
export async function openSidebarPage(t, label) {
  await t.click(`re=/^${label}\\b/`, { within: 'css=.ck-side' });
}

// ---------------------------------------------------------------- Tauri commands from the page (setup and checks only)

/** Call one of the app's own Tauri commands from the cockpit page, the way its UI does. */
export async function tauri(t, cmd, args = {}) {
  return t.page.eval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)})`);
}

// ---------------------------------------------------------------- an extra fake agent with a transcript ("ember")

const EMBER_TAIL = `
[session_chat]
format = "jsonl"
dir = "{cwd}/.ember"
file_glob = "*.jsonl"
pick = "newest_since_start"
write_tools = ["write_file"]

[session_chat.map]
role = "/role"
text = "/content"
timestamp = "/ts"
tool_name = "/name"
tool_input = "/input"
tool_id = "/id"
tool_result_for = "/for"
tool_output = "/output"
turn_end = "/stop"

[session_chat.values]
user = ["user"]
assistant = ["assistant"]
tool = ["tool"]
turn_end = ["end_turn"]
`;

/**
 * Install a second fake agent next to nimbus. `ember` has a Chat that reads a JSONL transcript (so turns end and
 * "Make clip" / "Branch" show on them); `label` and `id` are free. Writes the definition into the app's data folder,
 * copies the fake program under the name the definition asks for, then approves the definition through the bridge.
 */
export async function installAgent(t, { id = 'ember', label = 'Ember', transcript = true } = {}) {
  const nimbus = fs.readFileSync(new URL('../../fixtures/agents/nimbus.toml', import.meta.url), 'utf8');
  let text = nimbus.replace('id = "nimbus"', `id = "${id}"`).replace('label = "Nimbus"', `label = "${label}"`).replace('names = ["nimbus"]', `names = ["${id}"]`);
  if (transcript) text += EMBER_TAIL;
  const app = t.app;
  fs.writeFileSync(path.join(app.dataDir, 'agents', `${id}.toml`), text);
  const exe = process.platform === 'win32' ? `${id}.exe` : id;
  fs.copyFileSync(fakeTuiExe(), path.join(app.dirs.bin, exe));
  await t.bridge.post('/agents/definitions/reload', {});
  const defs = (await t.bridge.get('/agents/definitions')).body?.definitions || [];
  const def = defs.find((d) => d.id === id);
  if (!def) throw new Error(`the ${id} definition was not loaded: ${JSON.stringify(defs).slice(0, 400)}`);
  if (def.state !== 'active') {
    const a = await t.bridge.post('/agents/definitions/approve', { file: def.file, fingerprint: def.fingerprint });
    if (a.status !== 200) throw new Error(`approving ${id} failed: ${a.status} ${JSON.stringify(a.body).slice(0, 300)}`);
  }
  return id;
}

/** Append lines to a transcript file the ember definition reads (`<cwd>/.ember/<name>.jsonl`). */
export function transcriptLine(cwd, obj, name = 's1') {
  const dir = path.join(cwd, '.ember');
  fs.mkdirSync(dir, { recursive: true });
  fs.appendFileSync(path.join(dir, `${name}.jsonl`), JSON.stringify(obj) + '\n');
}

/**
 * A finished turn in an ember pane's Chat: start a pane in `cwd`, "type" the prompt (the fake agent plays its script),
 * and write the transcript lines a real CLI would write (the person's message, the answer, an end marker with times).
 * Returns the pane. The caller opens the Chat tab.
 */
export async function emberTurn(t, { cwd, prompt, answer = 'Done.', agent = 'ember', name = 's1', end = true }) {
  const pane = await t.spawnPane(agent, { cwd });
  const startedAt = Date.now();
  await pane.write(prompt);
  transcriptLine(cwd, { role: 'user', content: prompt, ts: startedAt }, name);
  await t.expect.eventually(async () => (await pane.screen()).includes('done in'), { timeout: 30000, message: 'expected the fake agent to finish the turn' });
  await waitIdle(t, pane);
  transcriptLine(cwd, end ? { role: 'assistant', content: answer, ts: Date.now(), stop: 'end_turn' } : { role: 'assistant', content: answer, ts: Date.now() }, name);
  return { pane, startedAt };
}

export async function openChatTab(t) {
  await t.click('exact=Chat');
  await t.waitFor('aria=Message to the agent');
}

// ---------------------------------------------------------------- fixtures shared inside one run

const built = new Map();
/**
 * Build a folder once per app process: the first test that asks for `key` runs `build(t)` (several finished turns take
 * about 12 s each), later tests in the same run get the same folder. A test run alone builds it itself. Use it only in
 * tests that do not change the folder's files.
 */
export async function sharedFolder(t, key, build) {
  const id = `${t.app.pid}|${key}`;
  if (!built.has(id)) built.set(id, await build(t));
  return built.get(id);
}

/** Select a pane of this folder (by the title of its first message), open its History and the What broke it sheet. */
export async function openWhatBroke(t, title) {
  await selectByTitle(t, title);
  await openHistory(t);
  await t.click('exact=What broke the tests?');
  await t.waitFor('Test command');
}

/**
 * Type the test command and save it, and wait until the sheet shows it as saved. When the folder already has a saved
 * command (another test in the run declared one) the sheet shows it with a Change button: press that first.
 */
export async function declareTest(t, command) {
  if (!(await t.page.has('css=.ck-sheet--wb input'))) await t.click('exact=Change', { nth: 0, within: 'css=.ck-sheet--wb' });
  await t.fill('css=.ck-sheet--wb input', command);
  await t.click('exact=Save');
  await t.expect.notVisible('css=.ck-sheet--wb input', { timeout: 20000 });
}

/** Press "Find it" once it is on (it needs a saved command and two save points). */
export async function findIt(t) {
  await t.expect.eventually(async () => !(await isDisabled(t, 'exact=Find it')), { message: 'expected Find it to be on' });
  await t.click('exact=Find it');
}

export const WB_SHEET = 'css=.ck-sheet--wb';

/** A folder with a test command that fails when `bad.flag` exists, and finished turns that write these files. */
export async function turnsFolder(t, name, turns, extraFiles = {}) {
  const cwd = gitProject(t, name, { 'check.mjs': CHECK_MJS, ...extraFiles });
  t.fake.script(turnsScript(turns));
  for (const turn of turns) await savedTurn(t, cwd, turn.prompt);
  return cwd;
}

/** True when the button/field at this target is disabled. */
export async function isDisabled(t, target, opts) {
  const r = await t.page.waitFor(target, opts);
  return t.page.eval(`(()=>{const el=document.elementFromPoint(${r.x + r.w / 2},${r.y + r.h / 2});const b=el&&el.closest('button,input,select,textarea');return !!b&&(b.disabled||b.getAttribute('aria-disabled')==='true')})()`);
}

/** Fixtures for What broke it. Each builds a git folder and finishes its turns (about 12 s per turn). */
export const FIXTURES = {
  // the second turn writes the file that makes the test command fail
  culprit: (t) => turnsFolder(t, 'culprit', [
    { prompt: 'add notes', writes: [['notes.txt', 'first notes']] },
    { prompt: 'add flag', writes: [['bad.flag', 'x']] },
  ]),
  // no turn breaks anything
  green: (t) => turnsFolder(t, 'green', [
    { prompt: 'add notes', writes: [['notes.txt', 'first notes']] },
    { prompt: 'add readme', writes: [['readme.txt', 'second']] },
  ]),
  // the first turn already breaks it, so the oldest save point is red
  red: (t) => turnsFolder(t, 'red', [
    { prompt: 'add flag', writes: [['bad.flag', 'x']] },
    { prompt: 'add notes', writes: [['notes.txt', 'later']] },
  ]),
};

/**
 * A pane in a folder that was built earlier (possibly by another test): its History only has its buttons once the pane
 * has a save point of its own, so the pane starts a long turn. Selected, ready for `openSheetOnSelected`.
 */
export async function paneInFolder(t, cwd) {
  return paneWithFirstSavePoint(t, cwd, 'look around the folder');
}

/** Open the What broke the tests sheet on a pane (the one selected). */
export async function openSheetOnSelected(t) {
  await openHistory(t);
  await t.click('exact=What broke the tests?');
  await t.waitFor('Test command');
}

/** A pane whose turn has started (one "before" save point, no "after" yet): History and its buttons are there. */
export async function paneWithFirstSavePoint(t, cwd, prompt = 'take a long look') {
  t.fake.script(turnsScript([{ prompt, think: 40000, say: 'finally' }]));
  const pane = await t.spawnPane('nimbus', { cwd });
  await pane.write(prompt);
  await waitSavePoint(t, pane, 'before');
  await selectByTitle(t, prompt);
  return pane;
}

// Helpers for the "real work" tests: a fake agent that writes, edits, deletes and runs things, and the app has to cope.
// Not a test (files that start with "_" are never run).
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { SkipTest } from '../../src/testctx.mjs';

export { calm, openPane, PANE_TITLE, SIDE, toasts, groupOfRow, sidebarGroups, sidebarTitles, resizeWindow, focusTerminal, paneHeading, slowScript } from '../app/_area.mjs';
export { gitProject, projectDir, git, writeFiles, waitIdle, savePoints, waitSavePoint, selectByTitle, openTab, tauri, openSidebarPage } from '../whatbroke/_area.mjs';
export { openChat, sendChat, dismissAway, installAgent, COMPOSER } from '../chat/_area.mjs';
export { PANE_ROW } from '../_helpers.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
export const SPAWN_HELPER = path.join(HERE, '_tools', 'spawn-helper.mjs');

// ---------------------------------------------------------------- scripts (pm-fake-tui TOML)

const q = (s) => JSON.stringify(String(s)); // a TOML basic string is a JSON string for our texts

/** Step builders. Each returns one TOML inline table (it must stay on one line). */
export const S = {
  think: (ms, verb) => `{ do = "think", ms = ${ms}${verb ? `, verb = ${q(verb)}` : ''} }`,
  wait: (ms) => `{ do = "wait", ms = ${ms} }`,
  say: (text) => `{ do = "say", text = ${q(text)} }`,
  write: (file, content, { append = false, quiet = false } = {}) => `{ do = "write_file", path = ${q(file)}, content = ${q(content)}${append ? ', append = true' : ''}${quiet ? ', quiet = true' : ''} }`,
  run: (command) => `{ do = "run", command = ${q(command)} }`,
  tool: (label, detail, ms = 0, failed) => `{ do = "tool", label = ${q(label)}, detail = ${q(detail)}, ms = ${ms}${failed ? `, failed = ${failed}` : ''} }`,
  lines: (count, template = 'row {n}') => `{ do = "lines", count = ${count}, template = ${q(template)} }`,
  link: (text, target, before = '', after = '') => `{ do = "link", text = ${q(text)}, target = ${q(target)}, before = ${q(before)}, after = ${q(after)} }`,
  exit: (code = 0) => `{ do = "exit", code = ${code} }`,
  crash: (message, code = 101) => `{ do = "crash", message = ${q(message)}, code = ${code} }`,
  hold: (text) => `{ do = "hold", text = ${q(text)} }`,
  /** An approval. kind: bash | edit | create. `allow` / `deny` are lists of steps (strings from this object). */
  approve: ({ kind = 'bash', command, path: p, allow = [], deny = [] }) =>
    `{ do = "approve", kind = ${q(kind)}${command ? `, command = ${q(command)}` : ''}${p ? `, path = ${q(p)}` : ''}, on_allow = [${allow.join(', ')}], on_deny = [${deny.join(', ')}] }`,
};

/**
 * A nimbus script from turns: `[{ on: 'prompt substring' | undefined, steps: [S.think(300), ...] }]`.
 * `header` is extra top-level TOML (`approvals = "menus"`).
 */
export function nimbus(turns, header = '') {
  let out = `agent = "nimbus"\nboot_ms = 100\ninline = true\n${header}\n`;
  for (const turn of turns) out += `[[turn]]\n${turn.on ? `on_prompt = ${q(turn.on)}\n` : ''}steps = [\n  ${turn.steps.join(',\n  ')},\n]\n`;
  return out;
}

/** One turn for every prompt. */
export const oneTurn = (steps, header = '') => nimbus([{ steps }], header);

// ---------------------------------------------------------------- waiting and reading

/** The prompts the fake agents logged since this test started. */
export const prompts = (t) => t.fake.events().filter((e) => e.ev === 'prompt').map((e) => String(e.text ?? ''));

export const waitStatus = (t, pane, status, timeout = 30000) => t.expect.eventually(async () => (await pane.info())?.status === status, { timeout, message: async () => `expected pane ${pane.id.slice(0, 8)} to be ${status}; it is ${(await pane.info())?.status}` });

export const waitRunning = (t, pane, timeout = 20000) => waitStatus(t, pane, 'running', timeout);

/** Type a task into the pane the way a person at the terminal does, and wait until the app says it works. */
export async function startTask(t, pane, text) {
  await pane.write(text);
  await waitRunning(t, pane);
}

/** Wait for the fake agent's `run` of a command that contains `fragment`; returns its event (`code`, `output`). */
export const waitRun = (t, fragment, timeout = 30000) => t.fake.waitEvent((e) => e.ev === 'run' && String(e.command).includes(fragment), { timeout });

/** The checkpoint list of a pane: `{ state, items }`, newest first. */
export async function checkpoints(t, pane) {
  const r = await t.bridge.get(`/panes/${pane.id}/checkpoints`);
  return r.body || {};
}

/** Wait for a save point of this kind and give back the whole list (newest first). */
export async function waitPoint(t, pane, kind = 'after', timeout = 70000) {
  return t.expect.eventually(async () => {
    const l = await checkpoints(t, pane);
    return (l.items || []).some((c) => c.kind === kind) ? l : null;
  }, { timeout, message: async () => `expected a "${kind}" save point; the pane has ${JSON.stringify(((await checkpoints(t, pane)).items || []).map((c) => c.label))}` });
}

/** The file changes of one save point, as the History tab expands them. */
export async function diffOf(t, pane, cp) {
  const r = await t.bridge.get(`/panes/${pane.id}/checkpoints/${cp.id}/diff`);
  return r.body?.changes || [];
}

/** Open the History tab of the open pane and wait for its rows (it reads the list when it opens: try again until the point is there). */
export async function openHistoryWith(t, text, timeout = 70000) {
  await t.expect.eventually(async () => {
    await t.click('exact=History');
    const end = Date.now() + 4000; // the tab reads its list when it opens: give it a moment before trying again
    while (Date.now() < end) {
      if ((await t.page.text()).includes(text)) return true;
      await t.sleep(300);
    }
    await t.click('exact=Terminal');
    return false;
  }, { timeout, interval: 700, message: `History lists "${text}"` });
}

/** True when the page text holds all of these words. */
export const pageHas = async (t, ...texts) => { const s = await t.page.text(); return texts.every((x) => s.includes(x)); };

/** The lines of the pane's screen that match a pattern, in order. */
export async function screenLines(pane, re) {
  return (await pane.screen()).split('\n').map((l) => l.trim()).filter((l) => re.test(l));
}

// ---------------------------------------------------------------- known bugs

/**
 * Run the checks of a test that looks for a real product bug. When they pass the bug is gone and the test passes. When they
 * fail the test ends as SKIPPED with the reason (the video and the log stay), so a known bug never turns a run red and is never
 * lost. `what` is a one-line description of the bug (it goes in the skip reason and in the report).
 */
export async function knownBug(t, what, checks) {
  try {
    await checks();
  } catch (e) {
    t.log('KNOWN BUG', what, '|', String(e.message).split('\n')[0]);
    throw new SkipTest(`known bug: ${what} (${String(e.message).split('\n')[0].slice(0, 160)})`);
  }
  t.log('the known bug did not show up', what);
}

// ---------------------------------------------------------------- the bridge from inside a pane

/** The command a fake agent runs to start a helper pane through the bridge (from inside its own pane). */
export const spawnHelperCmd = (t, { agent = 'nimbus', cwd, message = '' } = {}) => `node "${SPAWN_HELPER}" ${t.app.bridge.port} ${agent} "${cwd}" ${message}`.trim();

/**
 * One turn of work in a folder: start a fake agent pane there, give it the task, and wait until the turn is over and both
 * save points exist. The steps start with a few seconds of thinking so the app sees the pane working (it takes its "before"
 * save point then). Returns { pane, list }.
 */
export async function workTurn(t, { cwd, steps, prompt = 'do the work', header = '', think = 2500 } = {}) {
  t.fake.script(oneTurn([S.think(think), ...steps], header));
  const pane = await t.spawnPane('nimbus', { cwd });
  await pane.write(prompt);
  await waitStatus(t, pane, 'running');
  await waitStatus(t, pane, 'idle', 60000);
  const list = await waitPoint(t, pane, 'after');
  return { pane, list };
}

/** The rows of the expanded History point: ["M a.txt +2 −1", "A bin.dat binary"]. */
export const historyFiles = (t) => t.page.eval(`[...document.querySelectorAll('.ck-hist-files li')].map((li) => li.innerText.split(String.fromCharCode(10)).join(' ').split(String.fromCharCode(9)).join(' ').split(' ').filter(Boolean).join(' '))`);

/** Select the pane by the title of its first message, open History and expand the row with this title. */
export async function openHistoryPoint(t, paneTitle, rowTitle) {
  const { selectByTitle } = await import('../whatbroke/_area.mjs');
  await selectByTitle(t, paneTitle);
  await openHistoryWith(t, rowTitle);
  await t.click(rowTitle, { within: 'css=.ck-hist' });
  await t.waitFor('css=.ck-hist-files li');
}

// ---------------------------------------------------------------- streaming output

/** Steps of a reply that streams: `count` rows "<label> row N", one every `every` ms. */
export function streamSteps(count, label = 'stream', every = 200) {
  const out = [];
  for (let i = 1; i <= count; i++) out.push(S.say(`${label} row ${i}`), S.wait(every));
  return out;
}

/** The row numbers of "<label> row N" found in a screen text, in the order they appear. */
export const rowNumbers = (text, label = 'stream') => [...String(text).matchAll(new RegExp(`${label} row (\\d+)`, 'g'))].map((m) => Number(m[1]));

/**
 * True when the numbers on the screen are a clean tail of 1..count: consecutive, ascending, no row doubled or shuffled, and ending at
 * `count`. (The pane's screen text holds the visible rows only, so the first rows have scrolled off.)
 */
export const isCleanRun = (nums, count, min = 10) => nums.length >= min && nums[nums.length - 1] === count && nums.every((n, i) => i === 0 || n === nums[i - 1] + 1);

// ---------------------------------------------------------------- helpers started from inside a pane

/** Panes whose recorded parent is this pane. */
export async function helpersOf(t, pane) {
  return (await t.panes()).filter((p) => p.parent_pane_id === pane.id);
}

export const waitHelpers = (t, pane, n, timeout = 40000) => t.expect.eventually(async () => (await helpersOf(t, pane)).length >= n, { timeout, message: async () => `expected ${n} helper(s) of pane ${pane.id.slice(0, 8)}; the panes are ${JSON.stringify((await t.panes()).map((p) => [p.title, p.parent_pane_id?.slice(0, 6)]))}` });

/**
 * An orchestrator pane: a fake agent that, when told "start helpers", runs each command (they start helper panes through the
 * bridge from inside the pane, the way an agent's own tool would) and then keeps working for `holdMs`. The script the helpers
 * play is given by `helperTurns` ([{ on, steps }]); it is written after the orchestrator has started, so only the helpers read it.
 * Returns the orchestrator pane (not yet asked to start).
 */
export async function orchestrator(t, { commands, helperTurns, holdMs = 6000, cwd } = {}) {
  t.fake.script(nimbus([{ on: 'start helpers', steps: [S.think(500), ...commands.map((c) => S.run(c)), S.think(holdMs), S.say('helpers launched')] }]));
  const orch = await t.spawnPane('nimbus', { cwd: cwd || t.project('orchestrator') });
  t.fake.script(nimbus(helperTurns));
  return orch;
}

/** Ask the orchestrator to start its helpers and wait until n of them exist. */
export async function startHelpers(t, orch, n) {
  await orch.write('start helpers');
  await waitHelpers(t, orch, n);
}

/** The rows of the sidebar's "N helpers" chips, as text. */
export const helperChips = (t) => t.page.eval(`[...document.querySelectorAll('.ck-side .ck-helper-chip')].map((e) => e.innerText.trim())`);

/** The titles of the groups of helpers in the sidebar ("Helpers of <title>"). */
export const helperGroups = (t) => t.page.eval(`[...document.querySelectorAll('.ck-side [role=group][aria-label^="Helpers of"]')].map((e) => e.getAttribute('aria-label'))`);

/** The "Helper: ..." rows of the open pane's Chat: [{ text, status }]. */
export const chatHelperRows = (t) => t.page.eval(`[...document.querySelectorAll('[data-testid=pane-helper-row]')].map((e) => ({ text: e.innerText.split(String.fromCharCode(10)).join(' ').trim(), status: e.dataset.status }))`);

/** The title of a pane (its first message) once it has one. */
export const titleOf = (t, id) => t.expect.eventually(async () => (await t.panes()).find((p) => p.id === id)?.title || null, { timeout: 30000, message: 'the pane gets a title' });

export { fs, path };

// Helpers for the Review desk and Package check tests.
import fs from 'node:fs';
import path from 'node:path';
import { fakeTuiExe } from '../../src/paths.mjs';
import { gitProject, turnsScript, savedTurn, waitIdle, openSidebarPage, tauri, projectDir } from '../whatbroke/_area.mjs';

export * from '../whatbroke/_area.mjs';

/**
 * Make the Claude Code profile start the FAKE agent: a copy of the fake program named `claude` in the run's own program
 * folder (the app puts that folder first on its PATH). The test refuses to go on unless the pane shows the fake agent's
 * screen, so the real CLI can never be the one that starts.
 */
export function useFakeClaude(t) {
  const exe = process.platform === 'win32' ? 'claude.exe' : 'claude';
  const dest = path.join(t.app.dirs.bin, exe);
  if (!fs.existsSync(dest)) fs.copyFileSync(fakeTuiExe(), dest); // a copy that is running cannot be overwritten
}

/** A script where the Claude-shaped fake asks to run a command and says what happened after the answer. */
export const claudeAsks = (command, { allow = 'command ran', deny = 'command refused' } = {}) => `agent = "claude"
boot_ms = 100
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = ${JSON.stringify(command)}, on_allow = [{ do = "say", text = ${JSON.stringify(allow)} }], on_deny = [{ do = "say", text = ${JSON.stringify(deny)} }] },
]
`;

/** Turn the registry lookups off, so nothing here reaches the network (the look-alike check stays on). */
export async function lookupsOff(t) {
  await tauri(t, 'package_check_settings_set', { settings: { lookups: false } });
}

/** Start the fake Claude with this script and ask it something; it stops at its approval. */
export async function claudeAtApproval(t, command, opts) {
  useFakeClaude(t);
  t.fake.script(claudeAsks(command, opts));
  const pane = await t.spawnPane('claude', { cwd: projectDir(t, 'install'), waitFor: /Claude Code/ });
  await t.expect.eventually(async () => (await pane.screen()).includes('Claude Code v'), { message: 'the pane shows the FAKE Claude screen' });
  await pane.write('please install it');
  await t.expect.eventually(() => t.fake.events().some((e) => e.ev === 'approval_shown'), { timeout: 20000, message: 'the fake agent shows its approval' });
  return pane;
}

export { gitProject, turnsScript, waitIdle, openSidebarPage };

// ---------------------------------------------------------------- the Review desk

export const DESK = 'css=.ck-rd';
export const ROW = 'css=[data-rd="item"]';

/** Open the Review desk from the sidebar. */
export async function openDesk(t) {
  await openSidebarPage(t, 'Review');
  await t.waitFor(DESK);
}

/** The text of every row of the desk, riskiest first. */
export const deskRows = (t) => t.page.eval(`[...document.querySelectorAll('[data-rd="item"]')].map(r => r.innerText.split(String.fromCharCode(10)).join(' ').split('  ').join(' ').trim())`);

/** The heading line: "Review · 3" and the counts. */
export const deskHead = (t) => t.page.eval(`(document.querySelector('.ck-rd-head')?.innerText || '').split(String.fromCharCode(10)).join(' ').trim()`);

/** The position of the first row that holds this text, or -1. */
export async function rowIndex(t, text) {
  return (await deskRows(t)).findIndex((r) => r.includes(text));
}

/**
 * Mark every row of the desk "looks good" (a mark only: nothing is merged or changed) until it is empty, so a test starts
 * from an empty desk even when an earlier test in the same run left finished turns behind.
 */
export async function clearDesk(t) {
  await openDesk(t);
  for (let i = 0; i < 40; i++) {
    const rows = await deskRows(t);
    if (rows.length === 0) return;
    await t.click(ROW, { nth: 0 });
    await t.press('y');
    await t.expect.eventually(async () => (await deskRows(t)).length < rows.length, { timeout: 15000, message: 'a row goes away after Looks good' });
  }
}

/** Finish one turn (with its save points) in a folder: the agent writes these files. */
export async function writingTurn(t, cwd, prompt, writes) {
  t.fake.script(turnsScript([{ prompt, think: 6000, writes }]));
  return savedTurn(t, cwd, prompt);
}

/** The desk row of one agent turn in your folder (its key holds the pane id). Earlier tests leave rows behind, so each test finds its own. */
export const rowSel = (pane) => `css=[data-rd-row^="folder:${pane.id}"]`;

export const rowPosition = (t, pane) => t.page.eval(`[...document.querySelectorAll('[data-rd="item"]')].findIndex(r => (r.dataset.rdRow || '').startsWith('folder:${pane.id}'))`);

/** The text of one pane row, or null. */
export const rowText = (t, pane) => t.page.eval(`(()=>{const r=[...document.querySelectorAll('[data-rd="item"]')].find(r => (r.dataset.rdRow || '').startsWith('folder:${pane.id}'));return r?r.innerText.replace(/\\s+/g,' ').trim():null})()`);

/** Open the desk until the row of this pane is on it (see openDeskWith). */
export async function openDeskWithPane(t, pane, { tries = 12 } = {}) {
  for (let i = 0; i < tries; i++) {
    await openDesk(t);
    const end = Date.now() + 3000;
    while (Date.now() < end) {
      if ((await rowPosition(t, pane)) >= 0) return;
      await t.sleep(400);
    }
    await t.click('exact=Close');
    await t.sleep(800);
  }
  throw new Error(`no row of pane ${pane.id.slice(0, 8)} on the Review desk. Rows: ${JSON.stringify(await deskRows(t))}`);
}

/** The text of every prompt the fake agents logged. */
export const sentPromptTexts = (t) => t.fake.events().filter((e) => e.ev === 'prompt').map((e) => String(e.text ?? ''));

/**
 * Open the desk until a row holds this text. The desk reads the list when it opens, and a finished turn needs a moment
 * to be counted, so a desk opened too early is closed and opened again.
 */
export async function openDeskWith(t, text, { tries = 12 } = {}) {
  for (let i = 0; i < tries; i++) {
    await openDesk(t);
    const end = Date.now() + 3000;
    while (Date.now() < end) {
      if ((await deskRows(t)).some((r) => r.includes(text))) return;
      await t.sleep(400);
    }
    await t.click('exact=Close');
    await t.sleep(800);
  }
  throw new Error(`no row with "${text}" on the Review desk. Rows: ${JSON.stringify(await deskRows(t))}`);
}

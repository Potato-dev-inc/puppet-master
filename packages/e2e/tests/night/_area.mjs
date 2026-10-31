// Helpers shared by the tests of this wave's feature areas (night, labs, store, focus, linux, telegram, plans, keepgoing).
// Not a test: files that start with "_" are never run.
import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fakeTuiExe, copyFakeTui } from '../../src/paths.mjs';

/** Settings the app starts with when Night Shift is switched on in Labs. */
export const NIGHT_ON = { night_shift_enabled: true };

// ---------- layout-stable clicks ----------
// Menus and sheets fill in after they open (a Git check, the plans, the saved tests), so a row moves a few pixels.
// A click is by coordinates, so wait until the target holds still, then click.

/** Wait until the target stays at the same place for two looks in a row. */
export async function stable(t, target, opts = {}) {
  let prev = null;
  for (let i = 0; i < 40; i++) {
    const r = await t.page.waitFor(target, opts);
    if (prev && Math.abs(r.x - prev.x) < 1 && Math.abs(r.y - prev.y) < 1) return r;
    prev = r;
    await t.sleep(150);
  }
  throw new Error('the target never held still: ' + JSON.stringify(target));
}

/** Click once the target holds still. */
export async function sclick(t, target, opts = {}) {
  await stable(t, target, opts);
  return t.click(target, opts);
}

// ---------- projects ----------

/** A git project with one commit (what a Night Shift copy needs). `files` maps name to text. */
export function gitProject(t, name, files = { 'readme.txt': 'hello\n' }) {
  const dir = t.project(`${t.meta.id}/${name}`);
  for (const [f, c] of Object.entries(files)) {
    fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
    fs.writeFileSync(path.join(dir, f), c);
  }
  const git = (...a) => execFileSync('git', ['-c', 'user.name=Test', '-c', 'user.email=test@example.com', '-c', 'core.autocrlf=false', ...a], { cwd: dir, stdio: 'pipe' });
  git('init');
  git('add', '.');
  git('commit', '-m', 'first commit');
  return dir;
}

/** The text of a project file, or null. */
export function readFile(dir, name) {
  try { return fs.readFileSync(path.join(dir, name), 'utf8'); } catch { return null; }
}

// ---------- fake agent scripts ----------

/** An agent that writes `file` with `content`, then says `reply`. (TOML inline tables stay on one line.) */
export const writerScript = (file, content, reply = 'Night task finished', thinkMs = 300) => `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = ${thinkMs} },
  { do = "write_file", path = "${file}", content = "${content}" },
  { do = "say", text = "${reply}" },
]
`;

/** An agent that keeps working for `ms` (a long night task: it is still running when the test looks). */
export const slowScript = (ms = 120000) => `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = ${ms} },
  { do = "say", text = "slow task done" },
]
`;

// ---------- the new-agent menu and the Tonight sheet ----------

/** Open the "+" menu and wait for it to settle. */
export async function openNewMenu(t) {
  await sclick(t, 'aria=New agent');
  await stable(t, 'More agents…');
}

/**
 * Open the "+" menu, press one of its items and wait for `result` to show. The menu fills in while it is open (the Git
 * check, the suggestion for Pick for me), so a press can land on a row that has just moved: open it again and press again.
 */
export async function openFromMenu(t, item, result) {
  for (let attempt = 0; attempt < 4; attempt++) {
    await openNewMenu(t);
    await sclick(t, item);
    if (await t.expect.eventually(() => t.page.has(result), { timeout: 4000, message: 'the menu item to open its page' }).then(() => true, () => false)) return;
    await t.press('Escape');
  }
  await t.waitFor(result);
}

/** Open the Tonight sheet from the "+" menu and wait until it has loaded its plans and tests. */
export async function openTonight(t) {
  await openFromMenu(t, 'exact=Tonight…', 'exact=Start Night Shift');
  await t.waitFor('exact=Ready', { within: 'css=.ck-night-plans' });
  await stable(t, 'aria=Task 1');
}

/** Type the words of a task (1 is the first). */
export async function writeTask(t, n, text) {
  await t.fill(`aria=Task ${n}`, text);
}

/** Tick a plan under "Plans to use" by its name. */
export async function tickPlan(t, name) {
  await sclick(t, { text: name, within: 'css=.ck-night-plans' });
}

/** A local HH:MM `hours` from now. */
export function clockIn(hours) {
  const d = new Date(Date.now() + hours * 3600_000);
  const p = (n) => String(n).padStart(2, '0');
  return `${p(d.getHours())}:${p(d.getMinutes())}`;
}

/** Set a time field of the sheet: the value and the events a keyboard makes. */
export async function setTime(t, ariaLabel, hhmm) {
  await t.page.eval(`(() => {
    const el = document.querySelector('input[type=time][aria-label=${JSON.stringify(ariaLabel)}]');
    if (!el) throw new Error('no time field ${ariaLabel}');
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    set.call(el, ${JSON.stringify(hhmm)});
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
}

/**
 * Plan a one-task night on Nimbus from a git project and press Start. Returns { dir, stopAt }. The stop time is set
 * to four hours from now so the test does not depend on the time of day.
 */
export async function startOneTaskNight(t, { text = 'write the night file', name = 'night-app', files, script } = {}) {
  const dir = gitProject(t, name, files);
  await t.spawnPane('nimbus', { cwd: dir }); // the pane that tells the sheet which project to offer
  if (script) t.fake.script(script); // what the night's own agent plays
  await openTonight(t);
  await writeTask(t, 1, text);
  await tickPlan(t, 'Nimbus');
  const stopAt = clockIn(4);
  await setTime(t, 'Stop at', stopAt);
  await t.click('exact=Start Night Shift');
  return { dir, stopAt };
}

/** Open the Morning page from the sidebar chip (a finished night). */
export async function openMorning(t) {
  await sclick(t, 'Open Morning');
  await t.waitFor('exact=Morning');
}

// ---------- settings ----------

/** Open Settings and one of its pages. */
export async function openSettings(t, page = 'General') {
  await sclick(t, 'exact=Settings');
  await t.waitFor('css=.ck-settings-host');
  await sclick(t, `exact=${page}`);
}

/** The switch of a settings row, found by its label. */
export const switchOf = (label) => `css=button[role=switch][aria-label=${JSON.stringify(label)}]`;

/** True when a settings switch is on. */
export async function isOn(t, label) {
  await t.waitFor(switchOf(label));
  return t.page.eval(`document.querySelector(${JSON.stringify(`button[role=switch][aria-label=${JSON.stringify(label)}]`)}).getAttribute('aria-checked') === 'true'`);
}

/** Scroll a CSS selector's element to the middle of the page (the driver cannot reach an element far off screen). */
export async function scrollToCss(t, selector) {
  // A page that is still filling in (Settings loads its blocks one by one) moves what is above the control after the
  // first scroll, so scroll again until the control is on screen and holds still for two looks in a row.
  let prev = null;
  let holds = 0;
  for (let i = 0; i < 60; i++) {
    const r = await t.page.eval(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return null; el.scrollIntoView({ block: 'center', behavior: 'instant' }); const b = el.getBoundingClientRect(); return { y: Math.round(b.y), h: b.height, ih: innerHeight }; })()`);
    const ok = r && r.y >= 0 && r.y + r.h <= r.ih;
    holds = ok && prev === r.y ? holds + 1 : 0;
    if (holds >= 3) return;
    prev = r ? r.y : null;
    await t.sleep(150);
  }
}

/** Flip a settings switch by its label. */
export async function flip(t, label) {
  const css = `button[role=switch][aria-label=${JSON.stringify(label)}]`;
  const read = () => t.page.eval(`document.querySelector(${JSON.stringify(css)})?.getAttribute('aria-checked')`);
  await t.waitFor(switchOf(label));
  const before = await read();
  // Settings fills in block by block and moves the row while it does; press, and press again if the switch did not move.
  for (let attempt = 0; attempt < 4; attempt++) {
    await scrollToCss(t, css);
    await sclick(t, switchOf(label));
    const end = Date.now() + 2500;
    while (Date.now() < end) {
      if ((await read()) !== before) return;
      await t.sleep(150);
    }
  }
}

// ---------- fake claude and codex on the app's PATH ----------

/**
 * Put a copy of the fake agent where the app finds a program called `name` (the run's own bin folder is first on its
 * PATH), so the built-in Claude Code or Codex profile starts the fake. Returns a function that takes it away again.
 * The app is shared by the tests of a run: always call the returned function in a `finally`.
 */
export function installFakeProgram(t, name) {
  const target = path.join(t.app.dirs.bin, process.platform === 'win32' ? `${name}.exe` : name);
  try {
    fs.copyFileSync(fakeTuiExe(), target);
  } catch (e) {
    // A copy left by an earlier test can still be running (Windows will not overwrite it): it is the same fake, use it.
    if (!fs.existsSync(target)) throw e;
  }
  return () => { try { fs.unlinkSync(target); } catch { /* still running or already gone */ } };
}

// ---------- small reads ----------

/** Whether the visible button with exactly this label is disabled (undefined when there is none). */
export function isDisabled(t, label) {
  return t.page.eval(`(() => { const b = [...document.querySelectorAll('button')].find((x) => x.innerText.trim() === ${JSON.stringify(label)} && x.getBoundingClientRect().width > 0); return b ? b.disabled : undefined; })()`);
}

/** Whether a switch inside the Tonight sheet (by its aria-label) is on. */
export function sheetSwitchOn(t, label) {
  return t.page.eval(`document.querySelector('button[role=switch][aria-label=${JSON.stringify(label)}]')?.getAttribute('aria-checked') === 'true'`);
}

/** The words of the one line that explains why Start is off ("" when there is none). */
export function blockedLine(t) {
  return t.page.eval(`document.querySelector('.ck-night-blocked')?.innerText || ''`);
}

/** After the app has just started: wait until the helper pane the launcher used to open the window is gone. */
export async function waitClean(t) {
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'the launcher\'s helper pane to close' });
  await t.expect.text('No agents running.');
}

// ---------- keeping the tests of one run apart ----------
// The tests of one run share the app's data folder (a restart does not clear it). Night Shift keeps its run, its
// settings and its history there, so a night left by one test would be resumed or shown by the next one.

/** The files of the Night Shift store (one level below the store folder), as paths. */
function nightFiles(t) {
  const root = path.join(t.app.dataDir, 'night-shift');
  const out = [];
  const walk = (dir, depth) => {
    let names = [];
    try { names = fs.readdirSync(dir, { withFileTypes: true }); } catch { return; }
    for (const e of names) {
      const p = path.join(dir, e.name);
      if (e.isDirectory()) { if (depth < 2) walk(p, depth + 1); } else out.push(p);
    }
  };
  walk(root, 0);
  return out;
}

/** Stop a night that is still going and forget every record of earlier nights. Safe to call at any time. */
export async function resetNight(t) {
  await baseSetup(t);
  await t.page.eval(`(async () => { try { await window.__TAURI_INTERNALS__.invoke('night_stop'); } catch (e) { /* no night */ } return true; })()`).catch(() => {});
  for (const f of nightFiles(t)) { try { fs.unlinkSync(f); } catch { /* gone */ } }
}

/**
 * Change app settings the way the Settings window does (no restart needed; the cockpit follows the change event).
 * The first patch makes the app keep its own copy of the settings, which does not hold the harness's
 * `onboarding_done`, so the welcome would come back: every patch carries it. It also carries `away_minutes: 0`: the app
 * counts only a real keyboard or mouse as presence, so a slow video run would otherwise get a "While you were away"
 * page over the cockpit after five minutes.
 */
export async function setSettings(t, patch) {
  await t.page.eval(`window.__TAURI_INTERNALS__.invoke('settings_patch', { patch: ${JSON.stringify({ onboarding_done: true, away_minutes: 0, ...patch })} }).then(() => true)`);
}

/** Setup for a Night Shift test: the Labs switch on and no night left over from an earlier test. */
export async function nightSetup(t) {
  await resetNight(t);
  await setSettings(t, { night_shift_enabled: true });
  // The chip of the night before is still on the page (the files went away without an event): load the page again so a
  // finished chip on screen is always this test's own.
  await t.page.reload();
  await t.expect.text('Inbox');
}

/** Every Labs switch these tests use, off (the default), and no night left over. */
export async function labsOff(t) {
  await resetNight(t);
  await setSettings(t, { missions_enabled: false, night_shift_enabled: false, simple_mode: false });
}

/** Click a pane row in the sidebar by its name, so the app knows the project it is working on. */
export async function pickPane(t, name) {
  await sclick(t, `exact=${name}`, { within: 'css=.ck-side' });
}

/** Scroll a control into view (a tall sheet scrolls inside itself, out of reach of the driver), then click it. */
export async function clickCss(t, selector) {
  await scrollToCss(t, selector);
  await sclick(t, `css=${selector}`);
}

/** Choose a project for a Tonight task by the words of its entry in the list. */
export async function pickProject(t, n, label) {
  await t.page.eval(`(() => {
    const sel = document.querySelector(${JSON.stringify(`select[aria-label="Project for task ${n}"]`)});
    const opt = [...sel.options].find((o) => o.text.includes(${JSON.stringify(label)}));
    if (!opt) throw new Error('no project ' + ${JSON.stringify(label)} + ' in ' + [...sel.options].map((o) => o.text).join(', '));
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(sel, opt.value);
    sel.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
}

// ---------- the Agent Store ----------

/** Open the Agent Store from the plus menu and wait for the cards (the first load takes several seconds). */
export async function openStore(t) {
  await openFromMenu(t, 'More agents…', 'exact=Agent Store');
  await t.waitFor('css=.ck-store-grid', { timeout: 45000 });
}

/** The names and button words of the Store cards, as "Name:Button" (Button is "-" for a card without one). */
export function storeCards(t) {
  return t.page.eval(`[...document.querySelectorAll('.ck-store-card')].map((c) => c.querySelector('.ck-row-title').innerText + ':' + (c.querySelector('button')?.innerText ?? '-'))`);
}

// ---------- Focus Time ----------

const tauri = (t, cmd, args = {}) =>
  t.page.eval(`window.__TAURI_INTERNALS__.invoke(${JSON.stringify(cmd)}, ${JSON.stringify(args)}).then((v) => JSON.stringify(v ?? null)).catch((e) => 'ERR ' + String(e))`);

/** Call a desktop command the way the page does (for set-up and clean-up only; the tests act through the screen). */
export const invokeCommand = tauri;

/** Focus off, the default length 45 and the quiet switch on: what a new profile has. Used before and after a Focus test. */
export async function focusReset(t) {
  await baseSetup(t);
  await tauri(t, 'focus_end');
  await tauri(t, 'focus_settings_set', { settings: { default_minutes: 45, quiet_blocking: true } });
  // The summary of an earlier Focus waits as a card at the top of the Inbox until it is dismissed; it would push the
  // chips down while a test is aiming at them.
  await tauri(t, 'focus_summary_dismiss');
  await t.expect.eventually(async () => !(await t.page.text()).includes('While you focused'), { timeout: 5000, message: 'the old Focus summary to go away' }).catch(() => {});
  await t.sleep(300);
}

/** The minutes a Focus line says are left ("Focus · 44 min left" gives 44), or null. */
export async function focusMinutesLeft(t) {
  const text = await t.page.eval(`document.querySelector('[data-focus=on] .ck-focus-note-title')?.innerText ?? ''`);
  const m = /(\d+) min left/.exec(text);
  return m ? Number(m[1]) : null;
}

/**
 * Start of every test of these areas. The Settings window keeps its own copy of the settings (the first change makes it),
 * and that copy does not hold the harness's "welcome finished" flag. One stray click on the Inbox's "Set up your agents"
 * button would also bring the welcome back for every later test. Write the flag again, and reload if the welcome is up.
 */
export async function baseSetup(t) {
  await setSettings(t, {});
  if ((await t.page.text()).includes('Step 1 of 7')) {
    await t.page.reload();
    await t.expect.text('Inbox');
  }
}

/** Open the Chat tab of a pane by its sidebar name, waiting for the sidebar and the tabs to hold still. */
export async function openChatOf(t, name = 'Nimbus') {
  await pickPane(t, name);
  await sclick(t, 'exact=Chat');
  await t.waitFor('aria=Message to the agent');
}

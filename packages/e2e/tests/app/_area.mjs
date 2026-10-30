// Helpers shared by the desktop tests (app, panes, settings, sidebar). Small steps named after what a person does.
// A file whose name starts with "_" is never run as a test.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { connect } from '../../src/cdp.mjs';

export const SIDE = 'css=.ck-side';
/** The title in the header of the open pane (it turns into a field while it is renamed). */
export const PANE_TITLE = 'css=.ck-pane-head .ck-title';

/** The page targets (windows) of the running app, from its debugging port. */
export async function cdpTargets(t) {
  const r = await fetch(`http://127.0.0.1:${t.app.cdpPort}/json/list`, { signal: AbortSignal.timeout(3000) });
  return r.json();
}

/** The pop-out windows of panes (Individual mode): `{ paneId, url, webSocketDebuggerUrl }`. */
export async function paneWindows(t) {
  return (await cdpTargets(t))
    .filter((x) => x.type === 'page' && /[?&]terminal=1/.test(x.url) && /[?&]detached=1/.test(x.url))
    .map((x) => ({ ...x, paneId: new URL(x.url).searchParams.get('pane') }));
}

/** The text a pane's own window shows (its terminal), read over the debugging port. */
export async function paneWindowText(t, paneId) {
  const win = (await paneWindows(t)).find((w) => w.paneId === paneId);
  if (!win) return null;
  const conn = await connect(win.webSocketDebuggerUrl);
  try {
    await conn.send('Runtime.enable');
    return await conn.evalJs('document.body ? document.body.innerText : ""');
  } finally {
    conn.close();
  }
}

/** The sidebar as a person reads it: `[{ caption: 'Working', rows: ['Nimbus 1', ...] }, ...]` (the Inbox row has no caption). */
export async function sidebarGroups(t) {
  return t.page.eval(`[...document.querySelectorAll('.ck-side .ck-group')].map((g) => ({
    caption: (g.querySelector('.ck-caption')?.firstChild?.textContent || '').trim(),
    rows: [...g.querySelectorAll('.ck-row-title')].map((e) => e.innerText.trim()),
  }))`);
}

/** The titles of the pane rows in the sidebar, top to bottom. */
export const sidebarTitles = (t) => t.page.eval(`[...document.querySelectorAll('.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat) .ck-row-title')].map((e) => e.innerText.trim())`);

/** Which sidebar group (caption) lists a row with this title; null when it is not listed. */
export async function groupOfRow(t, title) {
  const g = (await sidebarGroups(t)).find((x) => x.rows.includes(title));
  return g ? g.caption : null;
}

/** Ctrl+K and wait for the box. */
export async function openPalette(t) {
  await t.press('Ctrl+K');
  await t.waitFor('aria=Search commands');
}

/** The titles of the rows the palette lists now. */
export const paletteTitles = (t) => t.page.eval(`[...document.querySelectorAll('.ck-palette-row .ck-palette-title')].map((e) => e.innerText.trim())`);

/** The open pane's title as shown in the header ('' when none is open). */
export const paneHeading = (t) => t.page.eval(`document.querySelector('.ck-pane-head .ck-title')?.innerText?.trim() || ''`);

/** "dark" or "light": the theme the cockpit is drawn in (its data-theme). */
export const themeNow = (t) => t.page.eval(`document.querySelector('.cockpit')?.dataset.theme || ''`);

/** An agent that thinks for `ms` before it answers, so the pane is Working for a while. */
export const slowScript = (ms = 6000, reply = 'finally done') => `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = ${ms} },
  { do = "say", text = "${reply}" },
]
`;

/** An agent that asks `Allow nimbus to ... [y/n]` on any prompt, so the pane waits for the person. */
export const askScript = `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [{ do = "say", text = "created" }], on_deny = [{ do = "say", text = "skipped" }] },
]
`;

/** Click inside the terminal of the open pane (so what is typed goes to the agent). */
export async function focusTerminal(t) {
  const r = await t.page.waitFor('css=.ck-term-slot');
  await t.click([Math.round(r.x + r.w / 2), Math.round(r.y + r.h / 2)]);
}

/**
 * Scroll an element into the middle of its scroll box. The page driver can only scroll to something it can find on
 * screen, not to a row under the sidebar footer or a field far below the fold; this one asks the page itself.
 */
export async function reveal(t, selector) {
  await t.page.eval(`document.querySelector(${JSON.stringify(selector)})?.scrollIntoView({ block: 'center' })`);
  await t.sleep(200);
}

/** Open a pane from the sidebar by its name (the row is scrolled into view first: with many panes it may be out of sight). */
export async function openPane(t, name) {
  await t.page.eval(`[...document.querySelectorAll('.ck-side .ck-row-title')].find((e) => e.innerText.trim() === ${JSON.stringify(name)})?.scrollIntoView({ block: 'center' })`);
  await t.sleep(150);
  await t.click(`exact=${name}`, { within: SIDE });
  await t.waitFor(PANE_TITLE);
}

/**
 * Keep the first-run welcome off. The runner clears the page's storage between tests, which also forgets that this
 * install has had a pane, and the welcome then covers the cockpit for a test that starts with no pane. Marking it
 * and loading the page again is what the app itself does once a pane has opened. (Call it first in every test.)
 */
export async function calm(t) {
  // The first test after an app start is not reset by the runner: a helper pane the launcher opened may still be there.
  for (const p of await t.panes()) await t.bridge.del(`/panes/${encodeURIComponent(p.id)}`).catch(() => {});
  await t.expect.eventually(async () => (await t.panes()).length === 0, { message: 'no panes at the start' });
  // The app shows a "While you were away" card over the main area when agents finish while nobody is at the keyboard
  // for 5 minutes, which a long run of tests reaches. Settings > General > "Away digest after" 0 turns that off.
  await t.page.eval(`window.__TAURI_INTERNALS__ ? window.__TAURI_INTERNALS__.invoke('settings_patch', { patch: { away_minutes: 0 } }).then(() => true, () => false) : false`).catch(() => false);
  await t.page.eval(`try { localStorage.setItem('pm.cockpit.everSpawned', '1'); } catch (e) {}`);
  await t.page.reload();
  await t.expect.text('Puppet Master');
  await t.expect.text('Inbox');
  try {
    if (/While you were away/.test(await t.page.text())) await t.click('exact=Got it');
  } catch { /* no card */ }
  // The mode and the theme are saved settings that outlive a test: a test that failed half way may have left them changed.
  await restoreOrchestration(t);
  await restoreDark(t);
}

/** Back to Orchestration mode if a test left the app in Individual mode. */
export async function restoreOrchestration(t) {
  try {
    if (/Panes are in their own windows\./.test(await t.page.text())) {
      await t.click('exact=Switch to Orchestration');
      await t.expect.noText('Panes are in their own windows.');
    }
  } catch { /* the page is gone */ }
}

/** Put the theme back to dark (the theme is a saved setting that outlives a test; call it from `finally`). */
export async function restoreDark(t) {
  try {
    if ((await themeNow(t)) === 'light') await t.click('aria=Toggle theme');
  } catch { /* the page is gone */ }
}

/** The pane tab that is selected now ("Terminal", "Chat", ...). */
export const selectedTab = (t) => t.page.eval(`document.querySelector('.ck-tabs-bar [role=tab][aria-selected=true]')?.innerText?.trim() || ''`);

/** The small notices at the bottom of the window (toasts). */
export const toasts = (t) => t.page.eval(`[...document.querySelectorAll('.ck-toast')].map((e) => e.innerText.trim())`);

/** Change the size of the app window (for resize tests) and wait until the page has the new width. */
export async function resizeWindow(t, width, height) {
  const conn = t.page.c;
  const w = await conn.send('Browser.getWindowForTarget');
  await conn.send('Browser.setWindowBounds', { windowId: w.windowId, bounds: { windowState: 'normal', width, height } });
  await t.expect.eventually(async () => Math.abs((await t.page.eval('innerWidth')) - width) < 40, { message: `the window is about ${width} px wide` });
}

/**
 * For a test that makes the runner restart the app afterwards (`seed`, `fresh`): forget the bridge port file. The file
 * of a killed app stays behind, and the launcher takes it for the new app's file; when other apps run at the same time
 * the new app gets another port and the launcher would talk to someone else's app. Without the file it waits for the
 * new app's own. (Call it in `finally`; the running app does not need the file.)
 */
export function forgetBridgePort(t) {
  try { fs.unlinkSync(path.join(t.app.dataDir, 'puppet-master.bridge.port')); } catch { /* none */ }
}

/**
 * Start the app's program a second time, the way a double click on its icon or a command line does. The running app
 * (same profile, same data folders) is told and the new process quits. Returns what the process did.
 */
export function launchAgain(t, args = []) {
  const d = t.app.dirs;
  const env = {
    ...process.env,
    PUPPET_MASTER_PROFILE: t.app.profile,
    APPDATA: d.appdata,
    LOCALAPPDATA: d.local,
    HOME: d.home,
    USERPROFILE: d.home,
    XDG_DATA_HOME: d.local,
    WEBVIEW2_USER_DATA_FOLDER: path.join(d.root, 'webview'),
    PM_WINDOW_ANCHOR: t.app.anchor,
  };
  const r = spawnSync(t.app.exe, args, { env, cwd: d.root, timeout: 30000, windowsHide: true, encoding: 'utf8' });
  return { status: r.status, signal: r.signal, error: r.error ? String(r.error.message) : null };
}

/** The separate Settings windows of the app (Individual mode opens one). */
export async function settingsWindows(t) {
  return (await cdpTargets(t)).filter((x) => x.type === 'page' && /[?&]settings=1/.test(x.url));
}


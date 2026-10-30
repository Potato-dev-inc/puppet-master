// Shared helpers of the desktop tests live in ../app/_area.mjs; these are the ones for Settings.
import fs from 'node:fs';
import path from 'node:path';
export * from '../app/_area.mjs';
import { reveal } from '../app/_area.mjs';

/** Open Settings from the sidebar (inline in the cockpit) and optionally go to one page of it. */
export async function openSettings(t, page) {
  await t.click('exact=Settings', { within: 'css=.ck-side' });
  await t.waitFor('css=.settings--inline');
  if (page) await goToPage(t, page);
}

/** Click a page in Settings' own list (General, Agents, ...). */
export async function goToPage(t, page) {
  await t.click(`exact=${page}`, { within: 'css=.st-nav, .st-side-foot' });
  await t.expect.eventually(async () => (await settingsTitle(t)) === page, { message: `the page title is ${page}` });
}

/** The big title of the Settings page that is open. */
export const settingsTitle = (t) => t.page.eval(`document.querySelector('.st-title h1')?.innerText?.trim() || ''`);

/** The folder where the app keeps the agent definitions of this run (its own private copy). */
export const agentsDir = (t) => path.join(t.app.dataDir, 'agents');

/** Write a definition file into the agents folder; returns a function that removes it again (call it in `finally`). */
export function writeDefinition(t, file, text) {
  const full = path.join(agentsDir(t), file);
  fs.writeFileSync(full, text);
  return () => {
    try { fs.unlinkSync(full); } catch { /* already gone */ }
  };
}

/** A made-up extra agent that reuses the program of the fake agent Nimbus (it is on the app's PATH). Every test uses its own id, so an approval given in one test never reaches another. */
export const definitionToml = (id, label) => `schema_version = 1

[agent]
id = "${id}"
label = "${label}"
description = "${label}: a made-up agent for the settings tests."
experimental = true

[binary]
names = ["nimbus"]
install_hint = "install ${id}"

[tui]
argv = []
idle_after_ms = 1500
quiet_idle_after_ms = 800

[tui.ready]
regex = '\\|\\s*>\\s*type a task'
not_regex = ['ctrl\\+c to stop']

[tui.busy]
working_regex = ['ctrl\\+c to stop']
idle_regex = ['\\|\\s*>\\s*type a task']
`;

export const ORBIT_TOML = definitionToml('orbit', 'Orbit');

/** What a field of Settings holds now, by its accessible name (a part of it is enough). */
export const fieldValue = (t, label) => t.page.eval(`document.querySelector('[aria-label*=' + ${JSON.stringify(JSON.stringify(label))} + ']')?.value ?? null`);

/** Scroll a field of Settings into view (the page driver cannot reach what is below the fold). */
export const revealField = (t, label) => reveal(t, `[aria-label*=${JSON.stringify(label)}]`);

/** Wait until a field holds this value. */
export async function expectField(t, label, value) {
  await t.expect.eventually(async () => (await fieldValue(t, label)) === value, { message: async () => `expected the field "${label}" to hold ${JSON.stringify(value)}, it holds ${JSON.stringify(await fieldValue(t, label))}` });
}

/** Pick an option of a drop-down in Settings by its label (the native list cannot be clicked over the debugging port). */
export async function chooseOption(t, label, optionText) {
  await revealField(t, label);
  const picked = await t.page.eval(`(() => {
    const s = document.querySelector('select[aria-label=${JSON.stringify(label)}]');
    const o = s && [...s.options].find((x) => x.text.trim() === ${JSON.stringify(optionText)});
    if (!o) return null;
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, o.value);
    s.dispatchEvent(new Event('change', { bubbles: true }));
    return o.text.trim();
  })()`);
  if (!picked) throw new Error(`no drop-down "${label}" with an option "${optionText}"`);
}

/** The option a drop-down of Settings shows now. */
export const chosenOption = (t, label) => t.page.eval(`(() => { const s = document.querySelector('select[aria-label=${JSON.stringify(label)}]'); return s ? s.selectedOptions[0]?.text.trim() : null; })()`);

/** Scroll a control of the Settings page into view by "exact=Text" or "aria=Label" (the page driver cannot reach below the fold). */
export async function revealControl(t, target) {
  const at = target.indexOf('=');
  const kind = target.slice(0, at);
  const value = target.slice(at + 1);
  await t.page.eval(`(() => {
    const kind = ${JSON.stringify(kind)}, value = ${JSON.stringify(value)};
    const el = [...document.querySelectorAll('.st-main button, .st-main [role=switch], .st-main input, .st-main select')].find((e) => (kind === 'aria' ? (e.getAttribute('aria-label') || '').includes(value) : (e.innerText || '').trim() === value));
    if (el) el.scrollIntoView({ block: 'center' });
  })()`);
  await t.sleep(200);
}

/** Click a control of the Settings page, scrolling to it first. */
export async function clickControl(t, target, opts) {
  await revealControl(t, target);
  await t.click(target, opts);
}

// Helpers for the search tests (a file that starts with "_" is never run as a test).
import fs from 'node:fs';
import path from 'node:path';
import { REPO } from '../../src/paths.mjs';
import { clickWhenStill, dismissAway } from '../chat/_area.mjs';

export * from '../chat/_area.mjs';

/**
 * Start the old-chats tests from nothing. The app keeps what was brought in (and the chats of closed panes) for the whole
 * run, so the test first removes the imported chats and puts aside any history folders an earlier test left in the home
 * folder (renamed, not deleted).
 */
export async function resetOldChats(t) {
  await t.page.eval(`window.__TAURI_INTERNALS__.invoke('chat_import_clear')`);
  for (const name of ['.claude', '.codex']) {
    const dir = path.join(t.app.dirs.home, name);
    if (fs.existsSync(dir)) fs.renameSync(dir, `${dir}-before-${Date.now()}`);
  }
}

/**
 * Put the synthetic old chats of the repo's history fixtures (Claude Code: 2 chats, Codex: 3 chats) where the agents
 * keep theirs: in the home folder of the test app (the app reads `HOME`, which the harness points at its own folder).
 */
export async function seedOldChats(t) {
  await resetOldChats(t);
  const tui = path.join(REPO, 'packages', 'app', 'src-tauri', 'src', 'tui');
  fs.cpSync(path.join(tui, 'claude', 'fixtures', 'history', 'projects'), path.join(t.app.dirs.home, '.claude', 'projects'), { recursive: true });
  fs.cpSync(path.join(tui, 'codex', 'fixtures', 'history', 'sessions'), path.join(t.app.dirs.home, '.codex', 'sessions'), { recursive: true });
}

/** Open "Search all chats" with its shortcut. */
export async function openSearch(t) {
  await dismissAway(t);
  await t.press('Ctrl+Shift+F');
  await t.waitFor('aria=Search all chats');
}

/**
 * Put text in the search box (replacing what is there). An empty box is typed into: select-all and Backspace on an empty
 * box would count as "Backspace on an empty box" and leave the search.
 */
export async function searchFor(t, text) {
  const box = 'aria=Search all chats';
  const current = await t.page.value(box);
  if (current) await t.page.fill(box, text);
  else if (text) await t.page.type(text, { into: box });
}

/** Open the "Bring my old chats" panel through the command palette. */
export async function openBringChatsPanel(t) {
  await dismissAway(t);
  await t.press('Ctrl+K');
  await t.page.type('old chats');
  await t.press('Enter');
}

/**
 * Open "Bring my old chats" and get to the list of what each agent kept. The app remembers the result of the last import
 * until it restarts, so the panel may open on that result (Look again) instead of on its first screen (Find my chats).
 */
export async function openBringChats(t) {
  await openBringChatsPanel(t);
  await clickPanelButton(t, await panelStart(t));
}

/** Waits for the panel to settle on its first screen ('Find my chats') or on the last result ('Look again'). */
export async function panelStart(t) {
  // The panel first draws its intro and then, when the app answers, the last result: wait until two looks agree.
  let first = null;
  let before = '';
  await t.expect.eventually(
    async () => {
      const buttons = await panelButtons(t);
      const now = buttons.includes('Find my chats') ? 'Find my chats' : buttons.includes('Look again') ? 'Look again' : '';
      const settled = now !== '' && now === before;
      before = now;
      first = now;
      return settled;
    },
    { message: 'the Bring my old chats panel is open', interval: 600 },
  );
  return first;
}

/** Click a button of the panel by its words. */
export async function clickPanelButton(t, label) {
  await clickWhenStill(t, { exact: label, within: 'css=.bc-panel' });
}

/** The words of the buttons in the Bring my old chats panel, in order. */
export async function panelButtons(t) {
  return t.page.eval(`[...document.querySelectorAll('.bc-panel button')].map((b) => b.innerText.trim())`);
}

/** Bring the old chats in: "Bring my old chats" > Find my chats > Add N chats. */
export async function bringOldChats(t) {
  await openBringChats(t);
  await clickWhenStill(t, 're=/Add \\d+ chats? to search/');
  await t.expect.text('Press Ctrl+Shift+F to find anything in them.', { timeout: 30000 });
}

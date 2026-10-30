// Helpers for the Branch from here tests.
import fs from 'node:fs';
import { gitProject, turnsScript, savedTurn, selectByTitle, openTab, waitSavePoint, paneWithFirstSavePoint } from '../whatbroke/_area.mjs';

export * from '../whatbroke/_area.mjs';

/** A folder with two finished turns by two agents: notes.txt first, then readme.txt. The last pane is "add readme". */
export async function twoTurnFolder(t, name = 'trip') {
  const cwd = gitProject(t, name, { 'README.md': 'trip planner\n' });
  t.fake.script(turnsScript([
    { prompt: 'add notes', writes: [['notes.txt', 'first notes']] },
    { prompt: 'add readme', writes: [['readme.txt', 'second file']] },
  ]));
  await savedTurn(t, cwd, 'add notes');
  await savedTurn(t, cwd, 'add readme');
  return cwd;
}

/** A pane with one finished turn (both save points). */
export async function oneTurnFolder(t, name = 'trip') {
  const cwd = gitProject(t, name, { 'README.md': 'trip planner\n' });
  t.fake.script(turnsScript([{ prompt: 'add notes', writes: [['notes.txt', 'first notes']] }]));
  await savedTurn(t, cwd, 'add notes');
  return cwd;
}

/** Open the History tab of the pane with this title and press "Branch from here" on its n-th row (0 = newest). */
export async function openBranchSheet(t, title, row = 0) {
  await selectByTitle(t, title);
  await openTab(t, 'History');
  await t.waitFor('exact=Branch from here');
  await t.click('exact=Branch from here', { nth: row });
  await t.waitFor('Save point:');
}

/** The names of files in a folder, without the app's own and git's folders. */
export const folderFiles = (dir) => fs.readdirSync(dir).filter((n) => n !== '.git' && n !== '.puppet-master').sort();

/** The branch copy pane the app started (its folder is a "bf-" copy under the project's .puppet-master). */
export async function copyPane(t) {
  return t.expect.eventually(async () => (await t.panes()).find((p) => /worktrees[\\/]bf-/.test(p.cwd)), { timeout: 30000, message: 'expected a pane in a branch copy' });
}

export { waitSavePoint, paneWithFirstSavePoint };

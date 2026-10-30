// Helpers for the Collision Alert tests: two agents working at the same time in one git folder.
import { gitProject, turnsScript, waitIdle } from '../whatbroke/_area.mjs';

export * from '../whatbroke/_area.mjs';

/**
 * Two fake agents that both change `shared.txt` after thinking for a few seconds (so the app already sees both of them
 * working when the file changes), then keep working for a long time. Returns the folder and the two panes. The agents
 * are asked for different things so their names in the alert differ ("edit alpha" and "edit beta").
 */
export async function twoWriters(t, { cwdA, cwdB, file = 'shared.txt', hold = 90000 } = {}) {
  const folderA = cwdA || gitProject(t, 'app', { [file]: 'base\n' });
  const folderB = cwdB || folderA;
  t.fake.script(turnsScript([
    { prompt: 'edit alpha', think: 4000, writes: [[file, 'alpha was here']], hold, say: 'alpha done' },
    { prompt: 'edit beta', think: 4000, writes: [[file, 'beta was here']], hold, say: 'beta done' },
  ]));
  const a = await t.spawnPane('nimbus', { cwd: folderA });
  const b = await t.spawnPane('nimbus', { cwd: folderB });
  await a.write('edit alpha');
  await b.write('edit beta');
  await t.expect.eventually(async () => (await a.info())?.status === 'running' && (await b.info())?.status === 'running', { message: 'both agents are working' });
  return { folder: folderA, a, b };
}

export const ALERT_TITLE = 'are working in the same folder'; // the Inbox card title: "<agent> and <agent> are working in the same folder"
export { waitIdle };

/** Poll for `ms` milliseconds: the text must not show up at any look. (A wait for something NOT to happen.) */
export async function neverShows(t, text, ms, { every = 1000 } = {}) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    const now = await t.page.text();
    if (now.toLowerCase().includes(String(text).toLowerCase())) throw new Error(`"${text}" showed up, but it should not`);
    await t.sleep(every);
  }
}

export const COLLISION_SWITCH = 'aria=Warn me when two agents change the same file';

/** The switch in Settings > Agents: its state as the page shows it. */
export async function switchIsOn(t) {
  await t.page.waitFor(COLLISION_SWITCH);
  return t.page.eval(`(()=>{const s=document.querySelector('[aria-label="Warn me when two agents change the same file"]');if(!s)return null;return s.getAttribute('aria-checked')==='true'||s.checked===true||s.getAttribute('aria-pressed')==='true'})()`);
}

/** The pane the app started in a Branch from here copy. */
export const copyPaneOf = (t) => t.expect.eventually(async () => (await t.panes()).find((p) => /worktrees[\\/]bf-/.test(p.cwd)), { timeout: 30000, message: 'expected a pane in a branch copy' });

/** Bring the switch into view (the Agents page is long) and press it. */
export async function pressCollisionSwitch(t) {
  await t.page.waitFor(COLLISION_SWITCH);
  await t.page.eval(`document.querySelector('[aria-label="Warn me when two agents change the same file"]').scrollIntoView({ block: 'center' })`);
  await t.sleep(300);
  await t.click(COLLISION_SWITCH);
}

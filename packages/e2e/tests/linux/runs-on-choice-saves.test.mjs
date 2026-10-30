// Choosing Linux for an agent is saved (to the app's own file) and survives leaving Settings; choosing Windows undoes it.
// No Linux program is started: only the choice is saved. needs-external: needs a real WSL 2 distro on the test PC.
import { chooseRunsOn, linuxState, openLinuxBlock, runsOn } from './_area.mjs';
import { openSettings } from '../night/_area.mjs';

export const meta = {
  id: 'linux.runs-on-choice-saves',
  title: 'Choosing where an agent runs is remembered and can be undone',
  area: 'linux',
  tags: ['linux', 'needs-external'],
  edge: true,
};

export async function run(t) {
  const state = await openLinuxBlock(t);
  if (state !== 'ready') return t.skip(`needs a WSL 2 Linux on the PC (it says: ${state})`);
  const distro = await t.page.eval(`document.querySelector('select[aria-label="Runs on for Claude Code"]').options[1]?.value ?? null`);
  if (!distro) return t.skip('Claude Code has no Linux choice here (the section says why in the row)');

  try {
    await t.step('Set Claude Code to run in Linux');
    await chooseRunsOn(t, 'Claude Code', distro);
    await t.expect.eventually(async () => (await runsOn(t, 'Claude Code')) === distro, { message: 'the choice is shown' });
    t.expect.equal(await runsOn(t, 'Codex CLI'), '', 'another agent is untouched');

    await t.step('Leave Settings and come back: the choice is still there');
    await t.click('exact=Inbox');
    await openLinuxBlock(t);
    t.expect.equal(await runsOn(t, 'Claude Code'), distro);
  } finally {
    await t.step('Put it back on Windows');
    await openSettings(t, 'Agents');
    await openLinuxBlock(t);
    await chooseRunsOn(t, 'Claude Code', '');
    await t.expect.eventually(async () => (await runsOn(t, 'Claude Code')) === '', { message: 'back on Windows' });
    t.expect.equal(await linuxState(t), 'ready');
  }
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';

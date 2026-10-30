// When WSL is missing, or has no Linux, the section gives the one command to run and a Check again button, and
// installs nothing itself. needs-external: this PC must lack WSL or a distro, so on a PC with Linux the test skips.
import { linuxState, openLinuxBlock } from './_area.mjs';

export const meta = {
  id: 'linux.missing-wsl-explains',
  title: 'Without WSL the section shows the command to run and never installs anything',
  area: 'linux',
  tags: ['linux', 'needs-external'],
  edge: true,
};

export async function run(t) {
  const state = await openLinuxBlock(t);
  if (state === 'ready') return t.skip('this PC has a Linux in WSL 2, so the missing-WSL words cannot be shown here');

  await t.step('The section names what is missing');
  if (state === 'not_installed') await t.expect.text('WSL is not installed.');
  if (state === 'no_distro') await t.expect.text('WSL has no Linux yet.');
  if (state === 'broken') await t.expect.text('WSL did not answer as expected.');

  await t.step('Check again reads WSL once more and the words stay the same');
  await t.click('exact=Check again');
  await t.expect.eventually(async () => (await linuxState(t)) === state, { message: 'the same state is shown again' });

  await t.step('There are no per-agent choices to make');
  await t.expect.notVisible('aria=Runs on for Claude Code');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';

// Settings > Agents > Run agents in Linux (WSL) says what WSL looks like on this PC in plain words, whatever it is.
// What it says depends on the PC running the test (WSL missing, no Linux installed, or a Linux listed), so the test
// checks that one of the four known states is shown with its own words. needs-external for a real WSL.
import { linuxState, openLinuxBlock } from './_area.mjs';

export const meta = {
  id: 'linux.section-shows-wsl-state',
  title: 'The Linux section explains what WSL is like on this PC in plain words',
  area: 'linux',
  tags: ['linux', 'needs-external'],
};

export async function run(t) {
  await t.step('Open Settings > Agents and find the Linux section');
  const state = await openLinuxBlock(t);
  t.log('WSL state on this PC: ' + state);
  t.expect.ok(['ready', 'not_installed', 'no_distro', 'broken'].includes(state), `a known state is shown (${state})`);

  await t.step('The section says what to do or what it found');
  if (state === 'ready') {
    await t.expect.visible('css=[aria-label="Linux distros"] li');
    await t.expect.text(/· WSL [12]/);
    await t.expect.text(/Check \S+/);
  } else if (state === 'not_installed') {
    await t.expect.text('WSL is not installed. To use Linux, open PowerShell as administrator and run:');
    await t.expect.text('Puppet Master does not install it for you.');
    await t.expect.visible('exact=Check again');
  } else if (state === 'no_distro') {
    await t.expect.text('WSL has no Linux yet. Run:');
    await t.expect.visible('exact=Check again');
  } else {
    await t.expect.text('WSL did not answer as expected.');
    await t.expect.visible('exact=Check again');
  }
  t.expect.equal(await linuxState(t), state, 'the state does not flip by itself');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';

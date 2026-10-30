// With a Linux available, every agent that can run there still starts on Windows until the person says otherwise, and
// the section warns that Linux is not a sandbox. needs-external: it needs a real WSL 2 distro on the PC running the test.
import { linuxState, openLinuxBlock, runsOn } from './_area.mjs';

export const meta = {
  id: 'linux.agents-run-on-windows-by-default',
  title: 'Each agent runs on Windows by default and the section warns that Linux is no sandbox',
  area: 'linux',
  tags: ['linux', 'needs-external'],
};

export async function run(t) {
  const state = await openLinuxBlock(t);
  if (state !== 'ready') return t.skip(`needs a WSL 2 Linux on the PC (it says: ${state})`);

  await t.step('The rows for Claude Code, Codex CLI and OpenCode are there');
  for (const agent of ['Claude Code', 'Codex CLI', 'OpenCode']) await t.expect.visible(`aria=Runs on for ${agent}`);

  await t.step('Each one is set to Windows');
  for (const agent of ['Claude Code', 'Codex CLI', 'OpenCode']) t.expect.equal(await runsOn(t, agent), '', `${agent} runs on Windows`);

  await t.step('The notes explain sign-in and safety');
  await t.expect.text('In Linux an agent has its own sign-in. Puppet Master never copies your Windows logins or keys into Linux.');
  await t.expect.text('Linux is not a sandbox: Linux programs can still read your Windows drives.');
  await t.expect.text('Helpers you talk to on your phone always run on Windows.');
  t.expect.equal(await linuxState(t), 'ready');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';

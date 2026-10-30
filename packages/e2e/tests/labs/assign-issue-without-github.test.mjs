// Labs > Assign an issue: with no GitHub sign-in the block says what is missing, watches nothing and offers only to add a
// repository or a trusted name. (The test never contacts GitHub. needs-external for a real gh sign-in.)
import { labsOff, openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'labs.assign-issue-without-github',
  title: 'Assign an issue says GitHub is not ready and watches nothing',
  area: 'labs',
  tags: ['labs', 'needs-external'],
  edge: true,
};

const SWITCH = 'button[role=switch][aria-label="Watch GitHub issues"]';

export async function run(t) {
  await openSettings(t, 'General');
  await t.waitFor(`css=${SWITCH}`);
  await scrollToCss(t, SWITCH);

  await t.step('The feature explains itself and is off');
  await t.expect.text('Puppet Master checks GitHub about once a minute with your own gh sign-in. Nothing is watched until you add a repository below.');
  t.expect.equal(await t.page.eval(`document.querySelector('${SWITCH}').getAttribute('aria-checked')`), 'false');

  await t.step('The GitHub row says what is missing');
  const signedIn = await t.page.eval(`document.body.innerText.includes('Signed in to GitHub')`);
  if (!signedIn) {
    await t.expect.text('Install the GitHub CLI and run gh auth login. Nothing is checked until then.');
    t.expect.equal(await t.page.eval(`[...document.querySelectorAll('button')].find((b) => b.innerText.trim() === 'Check now').disabled`), true, 'Check now is off');
  } else {
    t.log('this PC has a gh sign-in inside the test profile; the not-signed-in words are not shown');
  }

  await t.step('No repositories and no runs yet');
  await t.expect.text('No repositories yet.');
  await t.expect.text('No runs yet.');
  await t.expect.visible('exact=+ Add a repository');

  await t.step('Only you can start a run until you add a name');
  await t.expect.text('You can always start a run. Add GitHub names for people you trust.');
  await t.expect.visible('exact=+ Add a GitHub name');
  t.expect.equal(await t.page.eval(`document.querySelector('button[role=switch][aria-label="Anyone with write access to the repo"]').getAttribute('aria-checked')`), 'false');
}

export const setup = labsOff;

// Install shows the vendor's command first and asks; Cancel runs nothing. (The test never presses Run: no program is installed.)
import { clickCss, openStore, storeCards } from '../night/_area.mjs';

export const meta = {
  id: 'store.install-confirm-cancel',
  title: 'Install shows the exact command and Cancel installs nothing',
  area: 'store',
  tags: ['store'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await openStore(t);
  // Which agents are installed on the PC running the test decides which cards say Install; take the first that does.
  const card = (await storeCards(t)).find((c) => c.endsWith(':Install'));
  if (!card) return t.skip('every installable agent is already on this PC, so no card offers Install');
  const name = card.slice(0, -':Install'.length);

  await t.step(`Press Install on ${name}`);
  await clickCss(t, `button[aria-label="Install ${name}"]`);
  await t.waitFor(`exact=${name}`, { within: 'css=.ck-sheet' });
  await t.expect.text('Not on this PC yet. Install it from here, or add it once it is installed.');
  await t.expect.text('Install runs the command in a window you can watch.');
  await t.expect.text('Puppet Master never sees your password or key.');

  await t.step('Press the big Install button: it only asks first');
  await t.click(`exact=Install ${name}`, { within: 'css=.ck-sheet' });
  await t.expect.text('Puppet Master will run this in a window you can watch:');
  await t.expect.visible('exact=Copy command');
  await t.expect.visible('exact=Run');

  await t.step('Cancel: nothing runs and the detail is still open');
  await t.click('exact=Cancel');
  await t.expect.notVisible('exact=Run');
  await t.expect.notVisible('exact=Copy command');
  await t.expect.text('Not on this PC yet.');

  await t.step('Close the detail: the card still offers Install');
  await t.click('exact=Close');
  await t.expect.notVisible('css=.ck-sheet');
  t.expect.ok((await storeCards(t)).includes(`${name}:Install`), 'the card still says Install');
  t.expect.equal(t.fake.events().length, 0, 'no agent was started');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';

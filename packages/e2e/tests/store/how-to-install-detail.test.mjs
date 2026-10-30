// An agent Puppet Master cannot install shows a "How to install" sheet with the vendor's page; Escape closes it.
import { clickCss, openStore, storeCards } from '../night/_area.mjs';

export const meta = {
  id: 'store.how-to-install-detail',
  title: 'How to install opens a sheet with the vendor steps and Escape closes it',
  area: 'store',
  tags: ['store'],
  timeout: 90000,
};

export async function run(t) {
  await openStore(t);
  const card = (await storeCards(t)).find((c) => c.endsWith(':How to install'));
  if (!card) return t.skip('every manual-install agent is already on this PC');
  const name = card.slice(0, -':How to install'.length);

  await t.step(`Press How to install on ${name}`);
  await clickCss(t, `button[aria-label="How to install ${name}"]`);
  await t.waitFor({ exact: name, within: 'css=.ck-sheet' });

  await t.step('The sheet says who makes it, its licence, how to sign in and how it starts');
  await t.expect.text('Vendor:');
  await t.expect.text('License:');
  await t.expect.text('Sign-in happens in the agent\'s own pane. Puppet Master never sees your password or key.');
  await t.expect.text(`When you open ${name}, Puppet Master starts it with this command:`);
  await t.expect.text(`Puppet Master cannot install ${name} from here.`);
  await t.expect.text('We have not run this on Windows ourselves.');

  await t.step('There is no install button for this one');
  await t.expect.notVisible(`exact=Install ${name}`);

  await t.step('Escape closes the sheet and the grid is back');
  await t.press('Escape');
  await t.expect.notVisible('css=.ck-sheet');
  await t.expect.visible('exact=Agent Store');
}

// The welcome and the settings copy of the app must be in their normal state.
export { baseSetup as setup } from '../night/_area.mjs';

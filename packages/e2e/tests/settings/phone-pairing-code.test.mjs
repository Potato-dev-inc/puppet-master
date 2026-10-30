// Settings > Phone: "Pair a phone" makes a one-time code and its link; Refresh QR makes a new one. No phone is paired by it.
// needs-external: a real phone (and a tunnel) is needed to finish the pairing; this test goes as far as the desktop does alone.
import { calm, openSettings, clickControl } from './_area.mjs';

export const meta = {
  id: 'settings.phone-pairing-code',
  title: 'Pair a phone makes a one-time code on this PC and Refresh QR makes a new one',
  area: 'settings',
  tags: ['phone', 'needs-external'],
};

const codeShown = async (t) => (await t.page.text()).match(/Pairing code\s+([A-Z0-9]{8})/)?.[1] || null;

export async function run(t) {
  await calm(t);
  await openSettings(t, 'Phone');

  await t.step('No phone is paired and no code is shown yet');
  await t.expect.text('No phones paired yet.');
  await t.expect.text('The phone opens: Shown once you make a code');

  await t.step('Pair a phone makes a code');
  await clickControl(t, 'exact=Pair a phone');
  await t.expect.eventually(async () => (await codeShown(t)) !== null, { message: 'a pairing code of 8 characters is shown' });
  const first = await codeShown(t);
  await t.expect.text(`/pair/${first}`);
  await t.expect.text('Expires');

  await t.step('The page warns that this address only works on this computer');
  await t.expect.text('This address only works on this computer, not on your phone.');

  await t.step('Refresh QR: a different code, and the link follows it');
  await clickControl(t, 'exact=Refresh QR');
  await t.expect.eventually(async () => {
    const now = await codeShown(t);
    return now !== null && now !== first;
  }, { message: 'a new code replaced the first one' });
  const second = await codeShown(t);
  await t.expect.text(`/pair/${second}`);
  await t.expect.noText(`/pair/${first}`);

  await t.step('Still no phone is paired');
  await t.expect.text('No phones paired yet.');
}

// Settings > Phone: opening the page makes no code. A code (and its QR picture) exists only after "Pair a phone", and the
// bridge's secret never shows on the page.
import { openSettingsPage, guarded, invoke, reveal } from '../companion/_area.mjs';

export const meta = {
  id: 'phone.pair-code-on-request',
  title: 'The pairing code and QR appear only after "Pair a phone"; no secret is shown',
  area: 'phone',
  tags: ['smoke', 'security', 'needs-external'], // a real phone has to scan the QR; this checks everything the PC shows
};

const QR = 'css=canvas[aria-label="Mobile pairing QR code"]';

export const run = guarded(async (t) => {
  await t.step('Open Settings > Phone');
  await openSettingsPage(t, 'Phone', 'Pair a phone');
  await reveal(t, '[data-testid="pair-a-phone"]');

  await t.step('Before the button: no code, no QR picture, a hint about the one-time code');
  await t.expect.text('Makes a one-time code. It expires after a few minutes.');
  await t.expect.noText('Pairing code');
  t.expect.equal(await t.page.count(QR), 0, 'no QR picture is on screen');

  await t.step('Press Pair a phone');
  await t.click('css=[data-testid="pair-a-phone"]');

  await t.step('A code with an expiry and the QR picture show');
  await t.expect.text('Pairing code');
  await t.expect.text('Expires');
  await t.expect.visible(QR);
  const code1 = await t.page.eval(`document.querySelector('[data-testid="pairing-code"] code').innerText`);
  t.expect.match(code1, /^[A-Z0-9]{8}$/, 'an 8 character code');

  await t.step('The computer\'s secret is nowhere on the page');
  const text = await t.page.text();
  t.expect.ok(!text.includes(t.app.secret), 'the bridge secret is not shown');
  const html = await t.page.eval('document.documentElement.outerHTML');
  t.expect.ok(!html.includes(t.app.secret), 'nor hidden in the page');

  await t.step('Refresh QR makes a new code');
  await t.click('exact=Refresh QR');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('[data-testid="pairing-code"] code').innerText`)) !== code1, { message: 'expected a new code' });

  await t.step('"Watch and answer only" makes yet another code, bound to that choice');
  const code2 = await t.page.eval(`document.querySelector('[data-testid="pairing-code"] code').innerText`);
  await t.click('Watch and answer only');
  await t.expect.eventually(async () => (await t.page.eval(`document.querySelector('[data-testid="pairing-code"] code').innerText`)) !== code2, { message: 'expected a new code for the new choice' });
  void invoke;
});

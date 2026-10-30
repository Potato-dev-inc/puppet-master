// A phone redeems a code and shows in Settings > Phone > Paired phones; Revoke signs it out and its token stops working.
import { openSettingsPage, guarded, pairPhone, asPhone, reveal } from '../companion/_area.mjs';

export const meta = {
  id: 'phone.pair-list-revoke',
  title: 'A paired phone is listed with what it may do and Revoke signs it out',
  area: 'phone',
  tags: ['needs-external'], // the phone is played by a script that sends what a real phone sends
};

export const run = guarded(async (t) => {
  await t.step('A phone pairs with a code made on the computer');
  const phone = await pairPhone(t, { name: 'Maya\'s phone' });
  t.expect.equal(phone.reply.status, 200, `paired (${JSON.stringify(phone.reply.body).slice(0, 200)})`);
  t.expect.ok(phone.token, 'it got a device token');

  await t.step('The phone can use the bridge now');
  t.expect.equal((await asPhone(t, phone, { path: '/panes' })).status, 200);
  const me = await asPhone(t, phone, { path: '/pair/me' });
  t.expect.equal(me.body.local, false);
  t.expect.equal(me.body.caps.length >= 4 || me.body.caps.watch === true, true, 'full control');

  await t.step('Settings > Phone lists it with the four abilities');
  await openSettingsPage(t, 'Phone', 'Paired phones');
  await reveal(t, '[data-testid="paired-devices"]');
  await t.expect.text("Maya's phone");
  await t.expect.text('Active · expires if unused for 30 days');
  for (const ability of ['Watch', 'Answer', 'Use the terminal', 'Start tasks']) await t.expect.text(ability);

  await t.step('Press Revoke');
  await t.click('exact=Revoke', { within: 'css=[data-testid="paired-devices"]' });
  await t.expect.text('No phones paired yet.');

  await t.step('The phone is signed out at once');
  const after = await asPhone(t, phone, { path: '/panes' });
  t.expect.equal(after.status, 401);
  t.expect.equal(after.body.error, 'invalid_token');
});

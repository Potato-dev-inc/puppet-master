// In Settings > Phone each ability of a paired phone has a switch. Turning "Use the terminal" off applies at once: the next
// typing call from that phone is refused.
import { openSettingsPage, guarded, pairPhone, asPhone, reveal } from '../companion/_area.mjs';

export const meta = {
  id: 'phone.switch-off-an-ability',
  title: 'Switching off "Use the terminal" for a phone makes its next typing refused at once',
  area: 'phone',
  tags: ['security', 'needs-external'],
  edge: true,
};

export const run = guarded(async (t) => {
  const pane = await t.spawnPane('nimbus');
  const phone = await pairPhone(t, { name: 'Typist' });
  const type = () => asPhone(t, phone, { method: 'POST', path: `/panes/${pane.id}/input`, body: { text: 'from phone', append_newline: false } });

  await t.step('A full-control phone may type');
  t.expect.equal((await type()).status, 200);

  await t.step('Open its abilities and switch off "Use the terminal"');
  await openSettingsPage(t, 'Phone', 'Paired phones');
  await reveal(t, '[data-testid="paired-devices"]');
  await t.click('aria=Typist: Use the terminal');
  await t.expect.eventually(async () => (await type()).status === 403, { message: 'expected typing to be refused after the switch' });

  await t.step('The other abilities are untouched: it can still watch');
  t.expect.equal((await asPhone(t, phone, { path: '/panes' })).status, 200);

  await t.step('Switch it back on: typing works again');
  await t.click('aria=Typist: Use the terminal');
  await t.expect.eventually(async () => (await type()).status === 200, { message: 'expected typing to work again' });
});

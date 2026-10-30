// Pane menu > Interrupt sends Ctrl+C: a working agent stops its turn and is idle again.
import { calm, openPane, slowScript, toasts, groupOfRow } from './_area.mjs';

export const meta = {
  id: 'panes.interrupt-working-agent',
  title: 'Interrupt in the pane menu stops a working agent',
  area: 'panes',
  tags: ['states', 'interrupt'],
};

export async function run(t) {
  await calm(t);
  t.fake.script(slowScript(60000));
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');

  await t.step('Give the agent a long task: it is Working');
  await pane.write('longtask');
  await t.expect.eventually(async () => (await pane.info())?.status === 'running', { message: 'the app says running', timeout: 15000 });
  await t.expect.text(/Nimbus · longtask|longtask/);
  t.expect.equal(await groupOfRow(t, 'longtask'), 'Working', 'listed under Working');
  await t.expect.text('Working');

  await t.step('Pane menu > Interrupt');
  await t.click('aria=Pane actions');
  await t.click('Interrupt');
  await t.expect.eventually(async () => (await toasts(t)).some((x) => x.includes('Sent Ctrl+C')), { message: 'a notice says Ctrl+C was sent' });

  await t.step('The agent stopped its turn and is Idle');
  await t.fake.waitEvent('interrupted');
  await t.expect.eventually(async () => (await pane.info())?.status === 'idle', { message: 'the app says idle', timeout: 15000 });
  t.expect.equal(await groupOfRow(t, 'longtask'), 'Idle', 'listed under Idle');
  await t.expect.text('note: interrupted');
}

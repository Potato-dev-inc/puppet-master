// An agent that stops to ask shows Needs you in the header, the sidebar and the Inbox; answering clears it.
import { calm, openPane, askScript, groupOfRow } from './_area.mjs';

export const meta = {
  id: 'panes.needs-you-state',
  title: 'A pane that waits for an answer shows Needs you and goes back to Idle after it',
  area: 'panes',
  tags: ['states', 'smoke'],
};

export async function run(t) {
  await calm(t);
  t.fake.script(askScript);
  const pane = await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');

  await t.step('The agent asks for permission to create a file');
  await pane.write('make it');
  await t.fake.waitEvent('approval_shown');
  await t.expect.eventually(async () => (await pane.info())?.status === 'waiting_input', { message: 'the app says waiting', timeout: 15000 });

  await t.step('Header and sidebar say Needs you');
  await t.expect.text(/Needs you ·/);
  t.expect.equal(await groupOfRow(t, 'make it'), 'Needs you', 'the row is under Needs you');
  t.expect.equal(await t.page.eval(`document.querySelector('.ck-row--flat .ck-count')?.innerText`), '1', 'the Inbox counts one');

  await t.step('Answer yes in the terminal');
  await pane.write('y', { enter: false });
  await t.fake.waitEvent('approval_answer');

  await t.step('The pane is Idle again and the file was made');
  await t.expect.eventually(async () => (await pane.info())?.status === 'idle', { message: 'the app says idle', timeout: 15000 });
  await t.expect.text('Nimbus · Idle');
  t.expect.equal(await groupOfRow(t, 'make it'), 'Idle', 'the row moved to Idle');
  await t.expect.text('created');
}

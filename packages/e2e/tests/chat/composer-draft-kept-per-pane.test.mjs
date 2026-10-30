// Text typed but not sent stays in the box of its pane when you look at another pane and come back.
import { COMPOSER, openChat } from './_area.mjs';

export const meta = {
  id: 'chat.composer-draft-kept-per-pane',
  title: 'An unsent message is kept for its pane while you look at another one',
  area: 'chat',
  tags: ['composer', 'two-panes'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  await t.spawnPane('nimbus', { cwd: t.project('draft-a') });
  await t.spawnPane('nimbus', { cwd: t.project('draft-b') });

  await t.step('Write a message in the first pane and do not send it');
  await openChat(t, 'Nimbus 1');
  await t.page.type('half written thought', { into: COMPOSER });

  await t.step('The second pane has its own empty box');
  await openChat(t, 'Nimbus 2');
  await t.expect.value(COMPOSER, '');
  await t.page.type('other pane note', { into: COMPOSER });

  await t.step('Back in the first pane the text is still there');
  await openChat(t, 'Nimbus 1');
  await t.expect.value(COMPOSER, 'half written thought');

  await t.step('And the second pane kept its own');
  await openChat(t, 'Nimbus 2');
  await t.expect.value(COMPOSER, 'other pane note');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 0, 'nothing was sent');
}

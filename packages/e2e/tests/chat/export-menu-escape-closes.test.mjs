// The chat menu closes with Escape and when the person clicks elsewhere; it leaves the chat as it was.
import { scribeChat, said } from './_area.mjs';

export const meta = {
  id: 'chat.export-menu-escape-closes',
  title: 'The chat menu closes with Escape or a click elsewhere',
  area: 'chat',
  tags: ['export'],
  edge: true,
};

export async function run(t) {
  await scribeChat(t, 'export-esc', [said.user('Hello'), said.agent('Hi there.')]);
  await t.expect.text('Hi there.');

  await t.step('Open the menu and press Escape');
  await t.click('aria=Chat menu');
  await t.expect.visible('exact=Save as page');
  await t.press('Escape');
  await t.expect.notVisible('exact=Save as page');

  await t.step('Open it again and click the conversation');
  await t.click('aria=Chat menu');
  await t.expect.visible('exact=Save as page');
  await t.click('exact=Hi there.');
  await t.expect.notVisible('exact=Save as page');
  await t.expect.text('Hi there.');
}

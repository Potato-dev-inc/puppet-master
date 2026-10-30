// The chat menu: Save as page / Save as Markdown first show the file as a preview. Nothing is saved or sent until a file is picked.
import { scribeChat, said, clickWhenStill } from './_area.mjs';

export const meta = {
  id: 'chat.export-menu-preview',
  title: 'Save as Markdown shows the chat as the file will hold it, and Close leaves',
  area: 'chat',
  tags: ['export', 'smoke'],
};

export async function run(t) {
  await scribeChat(t, 'export', [said.user('Explain the plan'), said.agent('The **plan** is simple.')]);
  await t.expect.text('The plan is simple.');

  await t.step('Open the chat menu');
  await t.click('aria=Chat menu');
  await t.expect.visible('exact=Save as page');
  await t.expect.visible('exact=Save as Markdown');
  await t.expect.visible('exact=Go live…');

  await t.step('Save as Markdown: the preview shows the text of the file');
  await t.click('exact=Save as Markdown');
  await t.expect.text('One Markdown file with this chat. Nothing is uploaded.');
  await t.expect.text('**You**');
  await t.expect.text('Explain the plan');
  await t.expect.text('The **plan** is simple.');
  await t.expect.text('_Saved from Puppet Master_');
  await t.expect.visible('exact=Save as Markdown…');

  await t.step('Switch to the page format and back');
  await clickWhenStill(t, 'css=.ck-chat-export [role=tab]', { nth: 0 });
  await t.expect.visible('exact=Save as page…');
  await clickWhenStill(t, 'css=.ck-chat-export [role=tab]', { nth: 1 });
  await t.expect.visible('exact=Save as Markdown…');

  await t.step('Close without saving');
  await clickWhenStill(t, 'exact=Close');
  await t.expect.notVisible('exact=Save as Markdown…');
  await t.expect.visible('aria=Message to the agent');
}

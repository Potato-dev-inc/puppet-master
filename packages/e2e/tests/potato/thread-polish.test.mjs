// Potato Bot, the thread polish: the bot's words are Markdown (bold, a checklist with ticks, a code block with Copy), the
// thread has a "Today" line and a time line at the top, hovering a message shows Copy and Reply, and Reply puts a "> "
// quote in the message box. The brain is the fake brain, which answers "Fish heard: <your text>", so a message you type as
// Markdown comes back as Markdown. Nothing opens a window of its own.
import path from 'node:path';
import { makeCompanion, guarded } from '../companion/_area.mjs';

export const meta = {
  id: 'potato.thread-polish',
  title: 'The bot\'s reply is Markdown with a checklist and code; the thread has day and time lines; hovering shows Copy and Reply',
  area: 'potato',
  tags: ['smoke'],
  fresh: true,
  seed: { potato_bot_enabled: true },
  timeout: 120000,
};

export const run = guarded(async (t) => {
  await t.step('Make a companion called Fish and turn it into a bot');
  await makeCompanion(t, { name: 'Fish', avatar: 'bit' });
  await t.page.reload();
  await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
  await t.click('exact=Make Fish a bot');
  await t.waitFor('Message Fish');

  await t.step('Send a message written as Markdown: the fake brain says it back');
  await t.page.fill('Message Fish', 'Done.\n\n- ✓ **Archived** → 26 newsletters\n- ✓ **Replied** → 9 routine threads\n- [ ] **Flagged** → 1 from nora\n\n```sh\nnpm test\n```');
  await t.press('Enter');
  await t.expect.text('Fish heard: Done.');
  await t.expect.visible('css=.ck-potato-bubble--bot .ck-chat-md-checklist');

  await t.step('The reply is formatted: bold words, three checklist rows, a code block with Copy');
  t.expect.equal(await t.page.count('css=.ck-potato-bubble--bot li.ck-chat-md-task'), 3, 'three checklist rows');
  t.expect.equal(await t.page.count('css=.ck-potato-bubble--bot li.ck-chat-md-task[data-checked="true"]'), 2, 'two are ticked');
  t.expect.ok(await t.page.has('css=.ck-potato-bubble--bot strong'), 'bold words are bold');
  t.expect.ok(await t.page.has('css=.ck-potato-bubble--bot pre code'), 'a code block');
  t.expect.equal(await t.page.has('css=.ck-potato-bubble--bot .ck-chat-md-copy'), false, 'Copy shows only when the block is hovered');

  await t.step('The thread has a Today line and a time line at the top');
  await t.expect.visible('css=.ck-potato-sep--day');
  await t.expect.text('Today', { within: 'css=.ck-potato-sep--day' });
  await t.expect.visible('css=.ck-potato-sep--time');

  await t.step('Hover the bot\'s message: Copy and Reply show; Reply puts a quote in the box');
  await t.page.hover('css=.ck-potato-line--bot .ck-potato-bubble');
  await t.expect.visible('aria=Reply to message', { within: 'css=.ck-potato-line--bot' });
  await t.expect.visible('aria=Copy message', { within: 'css=.ck-potato-line--bot' });
  const shot = process.env.E2E_SCREENSHOT || path.join(t.outDir, 'potato-thread-polish.png');
  await t.page.screenshot(shot);
  t.log('screenshot: ' + shot);
  await t.click('aria=Reply to message', { within: 'css=.ck-potato-line--bot' });
  const quoted = await t.page.value('css=.ck-potato-composer textarea');
  t.expect.ok(String(quoted).startsWith('> Fish heard: Done.'), 'the box starts with the quote, got: ' + quoted);
});

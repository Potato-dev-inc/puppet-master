// Potato Bot, the helper: "work: add a readme" makes a card, nothing starts until Start, the companion's own (fake) agent
// then gets the task, and when it is done the row "Helper finished" and the bot's one answer appear. The brain is the fake
// brain (no model, no key, no network); the agent is the fake agent. Nothing here opens a window of its own.
import path from 'node:path';
import { makeCompanion, guarded, invoke, taskScript } from '../companion/_area.mjs';

export const meta = {
  id: 'potato.dm-reply-and-helper',
  title: 'A work request makes a helper card; Start runs the agent in the folder; the end is a row and one answer',
  area: 'potato',
  tags: ['smoke'],
  fresh: true,
  seed: { potato_bot_enabled: true },
  timeout: 180000,
};

const isPrompt = (e) => (e.ev === 'prompt' || e.event === 'prompt' || e.kind === 'prompt') && String(e.text ?? '').includes('add a readme');

export const run = guarded(async (t) => {
  await t.step('Make a companion called Fish and a project folder it may send a helper to');
  await makeCompanion(t, { name: 'Fish', avatar: 'bit' });
  const project = t.project('readme-demo');
  const remembered = await invoke(t, 'project_remember', { path: project });
  if (remembered.err) throw new Error('could not remember the folder: ' + remembered.err);
  // the helper's agent says its answer after a short think
  t.fake.script(taskScript('README.md added.', 600));
  await t.page.reload();

  await t.step('Open Potato Bot and make Fish a bot');
  await t.click('exact=Potato Bot', { within: 'css=.ck-side' });
  await t.click('exact=Make Fish a bot');
  await t.waitFor('Message Fish');

  await t.step('Say hello: the reply and the mood come back');
  await t.page.fill('Message Fish', 'hello');
  await t.press('Enter');
  await t.expect.text('Fish heard: hello');
  await t.expect.text('Genki');

  await t.step('Ask for work');
  await t.page.fill('Message Fish', 'work: add a readme');
  await t.press('Enter');
  await t.expect.text('Fish wants a helper: add a readme');
  await t.expect.text('Folder: readme-demo');

  await t.step('Nothing started: no agent got the task');
  await t.expect.visible('exact=Start', { within: 'css=.ck-potato-card' });
  await t.expect.visible('exact=Change folder', { within: 'css=.ck-potato-card' });
  await t.expect.visible('exact=Not now', { within: 'css=.ck-potato-card' });
  t.expect.equal(t.fake.events().filter(isPrompt).length, 0, 'no agent has the task before Start');

  await t.step('Save a picture of the card');
  const shot = process.env.E2E_SCREENSHOT || path.join(t.outDir, 'potato-helper.png');
  await t.page.screenshot(shot);
  t.log('screenshot: ' + shot);

  await t.step('Press Start: the agent gets the task');
  await t.click('exact=Start', { within: 'css=.ck-potato-card' });
  const prompt = await t.fake.waitEvent(isPrompt, { timeout: 40000 });
  t.expect.ok(String(prompt.text).includes('add a readme'), 'the agent got the task');
  await t.expect.text('Started a helper in readme-demo.');
  await t.expect.eventually(async () => !(await t.page.has('exact=Start', { within: 'css=.ck-potato-card' })), { message: 'the card has no buttons once it was answered' });

  await t.step('The helper ends: a row and one answer from the bot');
  await t.expect.text('Helper finished in readme-demo.', { timeout: 60000 });
  await t.expect.text('Fish heard: helper finished');
  await t.expect.visible('css=.ck-potato-note-open');
  t.expect.equal(await t.page.count('css=.ck-potato-bubble--bot'), 3, 'hello, the work request, and exactly one answer to the end of the helper');
});

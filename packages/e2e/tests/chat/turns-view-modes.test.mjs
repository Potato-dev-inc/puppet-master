// Summary keeps your messages and the final answers; Normal shows the agent's talk too; Verbose opens the tool rows.
import { scribeChat, said } from './_area.mjs';

export const meta = {
  id: 'chat.turns-view-modes',
  title: 'Summary, Normal and Verbose show more and more of a turn',
  area: 'chat',
  tags: ['turns', 'smoke'],
};

const pressed = (t, name) =>
  t.page.eval(`[...document.querySelectorAll('.ck-chat-modes button')].find((b) => b.innerText.trim() === ${JSON.stringify(name)}).getAttribute('aria-pressed')`);

export async function run(t) {
  await scribeChat(t, 'modes', [
    said.user('Fix the bug'),
    said.agent('Let me think about where it could be.', { final: false }),
    said.call('c1', 'read_file', { path: 'bug.py' }),
    said.result('c1', 'the buggy line'),
    said.agent('Fixed the bug in bug.py.'),
  ]);
  await t.expect.text('Fixed the bug in bug.py.');

  await t.step('Normal: the talk of the agent and the activity line');
  t.expect.equal(await pressed(t, 'Normal'), 'true');
  await t.expect.text('Let me think about where it could be.');
  await t.expect.text('Read 1 file');
  await t.expect.noText('the buggy line');

  await t.step('Summary: only your message, the activity line and the final answer');
  await t.click('exact=Summary');
  await t.expect.text('Summary: your messages, final answers and activity lines.');
  await t.expect.noText('Let me think about where it could be.');
  await t.expect.text('Fixed the bug in bug.py.');
  await t.expect.text('Fix the bug');

  await t.step('Verbose: the rows are open with their output');
  await t.click('exact=Verbose');
  await t.expect.text('Let me think about where it could be.');
  await t.expect.text('the buggy line');
  await t.expect.noText('Summary: your messages');
}

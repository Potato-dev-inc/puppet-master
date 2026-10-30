// The view mode is kept for each pane: leave the pane and come back, it is still Verbose; the other pane is still Normal.
import { installScribe, writeScribeChat, said, openChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-view-mode-remembered',
  title: 'Each pane keeps its own view mode',
  area: 'chat',
  tags: ['turns', 'two-panes'],
  edge: true,
  timeout: 90000,
};

const pressed = (t, name) =>
  t.page.eval(`[...document.querySelectorAll('.ck-chat-modes button')].find((b) => b.innerText.trim() === ${JSON.stringify(name)}).getAttribute('aria-pressed')`);

export async function run(t) {
  await installScribe(t);
  const a = await t.spawnPane('scribe', { cwd: t.project('modes-a') });
  const b = await t.spawnPane('scribe', { cwd: t.project('modes-b') });
  writeScribeChat(a, [said.user('first pane question'), said.agent('first pane answer')]);
  writeScribeChat(b, [said.user('second pane question'), said.agent('second pane answer')]);

  await t.step('Choose Verbose in the first pane');
  await openChat(t, 'scribe 1');
  await t.expect.text('first pane answer');
  await t.click('exact=Verbose');
  t.expect.equal(await pressed(t, 'Verbose'), 'true');

  await t.step('The second pane still shows Normal');
  await openChat(t, 'scribe 2');
  await t.expect.text('second pane answer');
  t.expect.equal(await pressed(t, 'Normal'), 'true');

  await t.step('Back in the first pane it is still Verbose');
  await openChat(t, 'scribe 1');
  await t.expect.text('first pane answer');
  t.expect.equal(await pressed(t, 'Verbose'), 'true');
}

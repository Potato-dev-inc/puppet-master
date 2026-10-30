// A message written while the agent waits for an answer is not typed into the question. It waits and says what to do first.
import { turnScript, think, hold, say, openChat, sendChat } from './_area.mjs';

export const meta = {
  id: 'chat.outbox-waits-behind-question',
  title: 'A message written while the agent asks a question waits until it is answered',
  area: 'chat',
  tags: ['outbox', 'questions'],
  edge: true,
  timeout: 90000,
};

export async function run(t) {
  t.fake.script(
    turnScript([think(300), hold('Overwrite the file? (y/n)')], { on_prompt: 'start' }) +
      `
[[turn]]
steps = [ ${think(200)}, ${say('now answering')} ]
`,
  );
  await t.spawnPane('nimbus');
  await openChat(t);
  await sendChat(t, 'start');

  await t.step('The agent asks a question and waits');
  await t.expect.text('Answer the agent');

  await t.step('A message written now is not typed into the question');
  await sendChat(t, 'please continue');
  await t.expect.text('Waiting for the agent. If it asks something, answer it first.');
  t.expect.equal(t.fake.events().filter((e) => e.ev === 'prompt').length, 1, 'only the first message reached the agent');

  await t.step('Answering the question frees the agent and the message goes in');
  await t.click('exact=Allow once');
  await t.fake.waitEvent((e) => e.ev === 'prompt' && e.text === 'please continue', { timeout: 30000 });
  await t.expect.text('now answering');
  await t.expect.noText('Waiting for the agent. If it asks something');
}

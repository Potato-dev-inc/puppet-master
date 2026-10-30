// Edge case: the agent program quits with an error code. The pane shows an Error state and says the agent is not running.
import { openChat, sendChat } from '../_helpers.mjs';

export const meta = {
  id: 'panes.agent-exits-with-error',
  title: 'A pane whose agent exits with an error shows the Error state',
  area: 'panes',
  tags: ['states'],
  edge: true,
};

export async function run(t) {
  t.fake.script(`
boot_ms = 100
inline = true
[[turn]]
on_prompt = "quit now"
steps = [
  { do = "say", text = "bye" },
  { do = "exit", code = 3 },
]
`);
  const pane = await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Ask the agent to quit');
  await sendChat(t, 'quit now');

  await t.step('The pane shows the Error state');
  await t.expect.text('The agent is not running.');
  await t.expect.text(/Nimbus · Error/);
  await t.expect.text('Needs you');

  t.expect.equal((await pane.info()).status, 'error', 'the app reports the pane as an error');
}

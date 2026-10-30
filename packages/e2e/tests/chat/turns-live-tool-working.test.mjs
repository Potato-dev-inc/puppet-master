// A tool call that has no result yet shows as live work ("Working · ..."); when the result and the answer come it turns into the summary.
import { scribeChat, said, writeScribeChat } from './_area.mjs';

export const meta = {
  id: 'chat.turns-live-tool-working',
  title: 'A running tool call shows as Working and settles into a summary line',
  area: 'chat',
  tags: ['turns'],
  edge: true,
};

export async function run(t) {
  const pane = await scribeChat(t, 'live-tool', [said.user('Build it'), said.call('c1', 'run_shell', { command: 'npm run build' })]);

  await t.step('The command runs: the chat says what it is doing');
  await t.expect.text('Working · run_shell npm run build');
  await t.expect.noText('Ran 1 command');

  await t.step('The result and the answer arrive');
  writeScribeChat(pane, [said.result('c1', 'built in 3s', 0), said.agent('The build is done.')]);
  await t.expect.text('The build is done.');
  await t.expect.text('Ran 1 command');
  await t.expect.noText('Working · run_shell');
}

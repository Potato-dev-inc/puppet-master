// A command that failed shows in the activity line as "(1 failed)" and its output is there when the row is opened.
import { scribeChat, said } from './_area.mjs';

export const meta = {
  id: 'chat.turns-failed-tool',
  title: 'A failed command is marked as failed in the activity line',
  area: 'chat',
  tags: ['turns'],
  edge: true,
};

export async function run(t) {
  await scribeChat(t, 'failed-tool', [
    said.user('Run the tests'),
    said.call('c1', 'run_shell', { command: 'npm test' }),
    said.result('c1', 'FAIL 2 tests broke', 2),
    said.call('c2', 'run_shell', { command: 'npm run lint' }),
    said.result('c2', 'lint clean', 0),
    said.agent('Two tests broke. The lint is clean.'),
  ]);

  await t.step('The line counts both commands and marks the failed one');
  await t.expect.text('Ran 2 commands (1 failed)');
  await t.expect.text('Two tests broke. The lint is clean.');

  await t.step('Open it: the rows show the commands');
  await t.click('re=/Ran 2 commands/');
  await t.expect.text('npm test');
  await t.expect.text('npm run lint');
}

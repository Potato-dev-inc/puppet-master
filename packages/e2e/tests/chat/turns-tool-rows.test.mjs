// The tool calls of a turn fold into one line ("Read 1 file · Wrote 1 file"); a click opens the rows.
import { scribeChat, said } from './_area.mjs';

export const meta = {
  id: 'chat.turns-tool-rows',
  title: 'Tool calls fold into one activity line that opens into rows',
  area: 'chat',
  tags: ['turns'],
};

export async function run(t) {
  await scribeChat(t, 'tools', [
    said.user('Add a greeting'),
    said.agent('Reading first.', { final: false }),
    said.call('c1', 'read_file', { path: 'main.py' }),
    said.result('c1', "print('hi')"),
    said.call('c2', 'write_file', { path: 'main.py', content: "print('hello')" }),
    said.result('c2', 'wrote 1 line'),
    said.agent('Done: main.py prints hello.'),
  ]);

  await t.step('One line stands for the tool calls');
  await t.expect.text('Read 1 file · Wrote 1 file');
  await t.expect.text('Done: main.py prints hello.');
  await t.expect.noText('wrote 1 line');

  await t.step('Open the line to see each call');
  await t.click('re=/Read 1 file/');
  await t.expect.text('read_file');
  await t.expect.text('write_file');
  await t.expect.text('main.py');

  await t.step('Close it again');
  await t.click('re=/Read 1 file/');
  await t.expect.noText('write_file');
}

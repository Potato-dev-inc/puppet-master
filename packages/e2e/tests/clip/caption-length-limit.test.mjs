// Clip It: the caption is one line of at most 140 characters, with any letters, quotes and emoji.
import { finishedChatTurn, makeClip } from './_area.mjs';

export const meta = {
  id: 'clip.caption-length-limit',
  title: 'The clip caption stops at 140 characters and keeps unicode text',
  area: 'clip',
  tags: ['clip'],
  edge: true,
  timeout: 120000,
};

export async function run(t) {
  await finishedChatTurn(t);
  await makeClip(t);

  await t.step('Type 200 characters');
  const long = 'x'.repeat(200);
  await t.fill('css=.ck-clip-input', long);
  const value = await t.page.value('css=.ck-clip-input');
  t.expect.equal(value.length, 140, 'only 140 characters fit');

  await t.step('A caption with accents, quotes and a symbol is kept as typed');
  const odd = 'Zażółć "gęślą" jaźń — ünïcode ✓';
  await t.fill('css=.ck-clip-input', odd);
  await t.expect.value('css=.ck-clip-input', odd);
  await t.expect.visible('exact=Draw again');
}

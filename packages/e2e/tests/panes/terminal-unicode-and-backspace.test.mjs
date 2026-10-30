// Edge case: accents, symbols and CJK text go through the terminal intact; Backspace removes the last character.
import { calm, openPane, focusTerminal } from './_area.mjs';

export const meta = {
  id: 'panes.terminal-unicode-and-backspace',
  title: 'Unicode typed in the terminal arrives intact and Backspace edits it',
  area: 'panes',
  tags: ['terminal'],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus');
  await openPane(t, 'Nimbus');
  await focusTerminal(t);

  await t.step('Type text with accents, a symbol and Japanese, plus one wrong letter');
  await t.type('café ✓ 日本語x');
  await t.press('Backspace');
  await t.expect.text('> café ✓ 日本語');

  await t.step('Send it: the agent got the cleaned text');
  await t.press('Enter');
  const prompt = await t.fake.waitEvent('prompt');
  t.expect.equal(prompt.text, 'café ✓ 日本語', 'unicode arrived and the Backspace was honoured');
}

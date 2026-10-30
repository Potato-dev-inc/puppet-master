// In a split view Ctrl+Alt+Right and Ctrl+Alt+Left move the keyboard between the two chats, each with its own message box.
import { calm, openPane } from './_area.mjs';

export const meta = {
  id: 'panes.split-focus-keys',
  title: 'Ctrl+Alt+Right and Ctrl+Alt+Left move the focus between the two chats of a split',
  area: 'panes',
  tags: ['split', 'keyboard'],
  edge: true,
};

const focusedSide = (t) => t.page.eval(`document.querySelector('.ck-split-side.is-focused')?.dataset.splitSide || ''`);
const composerHasFocus = (t, side) => t.page.eval(`document.activeElement === document.querySelector('.ck-split-side[data-split-side=${side}] textarea')`);

export async function run(t) {
  await calm(t);
  await t.spawnPane('nimbus', { cwd: t.project('alpha') });
  await t.spawnPane('nimbus', { cwd: t.project('beta') });
  await openPane(t, 'Nimbus 2');
  await t.press('Ctrl+\\');
  await t.waitFor('css=.ck-split');

  await t.step('The left chat has the focus when the split opens');
  t.expect.equal(await focusedSide(t), 'left');

  await t.step('Ctrl+Alt+Right moves to the right chat and its message box');
  await t.press('Ctrl+Alt+ArrowRight');
  await t.expect.eventually(async () => (await focusedSide(t)) === 'right', { message: 'the right side is focused' });
  await t.expect.eventually(() => composerHasFocus(t, 'right'), { message: 'the cursor is in the right message box' });

  await t.step('Ctrl+Alt+Left moves back');
  await t.press('Ctrl+Alt+ArrowLeft');
  await t.expect.eventually(async () => (await focusedSide(t)) === 'left', { message: 'the left side is focused' });
  await t.expect.eventually(() => composerHasFocus(t, 'left'), { message: 'the cursor is in the left message box' });

  await t.step('Clicking a side focuses it as well');
  await t.click('aria=Message to the agent', { nth: 1 });
  await t.expect.eventually(async () => (await focusedSide(t)) === 'right', { message: 'a click on the right side focuses it' });
}

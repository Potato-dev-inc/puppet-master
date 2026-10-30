// Settings save by themselves: a number typed in Budgets is kept (also after a reload); a wrong entry is put right.
import { calm, openSettings, expectField, revealField } from './_area.mjs';

export const meta = {
  id: 'settings.budget-autosave',
  title: 'A number typed in Budgets saves by itself and bad input is corrected',
  area: 'settings',
  tags: ['autosave'],
};

const TURNS = 'Turns an agent may use';

/** Type a value into the turns field and press Enter. */
async function enter(t, value) {
  await revealField(t, TURNS);
  await t.fill(`aria=${TURNS}`, value);
  await t.press('Enter');
}

export async function run(t) {
  await calm(t);
  try {
    await openSettings(t, 'General');
    await revealField(t, TURNS);
    await expectField(t, TURNS, '0');

    await t.step('Type 7 turns and press Enter');
    await enter(t, '7');

    await t.step('Reload the window and come back: still 7');
    await t.page.reload();
    await t.expect.text('Puppet Master');
    await openSettings(t, 'General');
    await expectField(t, TURNS, '7');

    await t.step('A negative number becomes 0 (no limit)');
    await enter(t, '-5');
    await expectField(t, TURNS, '0');

    await t.step('A huge number is cut to the largest allowed');
    await enter(t, '99999');
    await expectField(t, TURNS, '10000');

    await t.step('Empty input puts the saved value back');
    await enter(t, '');
    await expectField(t, TURNS, '10000');
  } finally {
    // The setting outlives the test: put it back to "no limit".
    try {
      await enter(t, '0');
    } catch { /* Settings is not open */ }
  }
}

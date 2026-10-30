// Settings > General > Focus Time: the default length (5 to 240 minutes) marks one chip in the Inbox; bad numbers are fixed.
import { focusReset, openSettings, scrollToCss } from '../night/_area.mjs';

export const meta = {
  id: 'focus.settings-default-length',
  title: 'The default Focus length is kept between 5 and 240 and marks a chip',
  area: 'focus',
  tags: ['focus'],
  edge: true,
};

const FIELD = 'input[aria-label="Default length (minutes)"]';
const field = (t) => t.page.eval(`document.querySelector('${FIELD}').value`);
const defaultChip = (t) => t.page.eval(`document.querySelector('[data-focus=off] [data-default=true]')?.innerText ?? ''`);

async function commit(t, text) {
  await scrollToCss(t, FIELD);
  await t.fill(`css=${FIELD}`, text);
  await t.press('Enter');
}

export async function run(t) {
  try {
    await t.step('The Inbox marks 45 min as the default');
    t.expect.equal(await defaultChip(t), '45 min');

    await t.step('Open Settings > General > Focus Time');
    await openSettings(t, 'General');
    await scrollToCss(t, FIELD);
    t.expect.equal(await field(t), '45');

    await t.step('Set 90: the Inbox chip for 90 is marked');
    await commit(t, '90');
    await t.expect.eventually(async () => (await field(t)) === '90', { message: 'the field shows 90' });
    await t.click('exact=Inbox');
    await t.expect.eventually(async () => (await defaultChip(t)) === '90 min', { message: 'the 90 min chip is marked' });

    await t.step('A number that is too big is cut to 240');
    await openSettings(t, 'General');
    await commit(t, '999');
    await t.expect.eventually(async () => (await field(t)) === '240', { message: '999 becomes 240' });

    await t.step('A number that is too small is raised to 5');
    await commit(t, '1');
    await t.expect.eventually(async () => (await field(t)) === '5', { message: '1 becomes 5' });

    await t.step('Text that is not a number puts the saved value back');
    await commit(t, 'abc');
    await t.expect.eventually(async () => (await field(t)) === '5', { message: 'still 5' });
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;

// The Settings search finds a Labs switch by a word in its name or its explanation, and says so when nothing matches.
import { labsOff, openSettings } from '../night/_area.mjs';

export const meta = {
  id: 'labs.settings-search-finds-a-lab',
  title: 'Searching Settings finds Night Shift and says when nothing matches',
  area: 'labs',
  tags: ['labs'],
  edge: true,
};

const results = (t) => t.page.eval(`document.querySelector('.ck-settings-host').innerText`);

export async function run(t) {
  await openSettings(t, 'General');
  await t.waitFor('css=input[type=search]');

  await t.step('Search for "overnight": the Night Shift switch is the result');
  await t.fill('css=input[type=search]', 'overnight');
  await t.expect.eventually(async () => (await results(t)).includes('Search results'), { message: 'a results list' });
  await t.expect.visible('css=button[role=switch][aria-label="Night Shift"]');
  await t.expect.text('Plan tasks for tonight from the + menu.');

  await t.step('Other Labs rows are not in the results');
  await t.expect.eventually(async () => !(await results(t)).includes('Missions (Preview)'), { message: 'Missions is not listed' });

  await t.step('A word nobody uses: the page says nothing matches');
  await t.fill('css=input[type=search]', 'zzzqqq');
  await t.expect.text('No settings match “zzzqqq”.');

  await t.step('Clear the search: the normal page is back');
  await t.fill('css=input[type=search]', '');
  await t.expect.eventually(async () => !(await results(t)).includes('Search results'), { message: 'the results go away' });
  await t.expect.text('Early features');
}

export const setup = labsOff;

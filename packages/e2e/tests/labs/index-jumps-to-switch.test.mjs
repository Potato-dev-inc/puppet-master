// A name in the Labs list jumps to its own switch further down the page.
import { labsOff, openSettings, scrollToCss, switchOf } from '../night/_area.mjs';

export const meta = {
  id: 'labs.index-jumps-to-switch',
  title: 'Pressing a name in the Labs list scrolls to that feature\'s switch',
  area: 'labs',
  tags: ['labs'],
};

const onScreen = (t, label) =>
  t.page.eval(`(() => { const b = document.querySelector('button[role=switch][aria-label=${JSON.stringify(label)}]'); if (!b) return false; const r = b.getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight; })()`);

export async function run(t) {
  await openSettings(t, 'General');
  await t.expect.text('Early features');

  await t.step('Press Night Shift in the list');
  await scrollToCss(t, '.st-labs-list');
  await t.click({ exact: 'Night Shift', within: 'css=.st-labs-list' });

  await t.step('Its switch is now on screen');
  await t.expect.eventually(() => onScreen(t, 'Night Shift'), { message: 'the Night Shift switch scrolls into view' });
  await t.expect.visible(switchOf('Night Shift'));

  await t.step('Scroll back up and jump to Missions');
  // The page is still filling in below the list, so the first jump can land short; press again like a person would.
  let seen = false;
  for (let attempt = 0; attempt < 3 && !seen; attempt++) {
    await scrollToCss(t, '.st-labs-list');
    await t.click({ exact: 'Missions', within: 'css=.st-labs-list' });
    seen = await t.expect.eventually(() => onScreen(t, 'Missions (Preview)'), { timeout: 4000, message: 'the Missions switch scrolls into view' }).then(() => true, () => false);
  }
  t.expect.ok(seen, 'the Missions switch scrolls into view');
}

export const setup = labsOff;

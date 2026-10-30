// "Coming later" lists what is planned and says plainly that none of it works yet: no switches, no fields.
import { calm, openSettings } from './_area.mjs';

export const meta = {
  id: 'settings.coming-later-is-read-only',
  title: 'The Coming later page lists planned features and has no controls',
  area: 'settings',
  tags: [],
  edge: true,
};

export async function run(t) {
  await calm(t);
  await openSettings(t);

  await t.step('Open Coming later');
  await t.click('exact=Coming later', { within: 'css=.st-side-foot' });
  await t.expect.text('Not built yet. None of these do anything today.');

  await t.step('The planned features are listed, each one marked as not available');
  for (const item of ['Desktop notifications', 'Rules and guardrails', 'Backup and sync', 'Agent marketplace', 'Automation', 'Observability']) await t.expect.text(item);
  await t.expect.count('exact=Not available yet', 6);

  await t.step('Nothing on the page can be switched or typed into');
  const controls = await t.page.eval(`document.querySelectorAll('.st-main input, .st-main select, .st-main textarea, .st-main [role=switch], .st-main button').length`);
  t.expect.equal(controls, 0, 'no controls in the page');
}

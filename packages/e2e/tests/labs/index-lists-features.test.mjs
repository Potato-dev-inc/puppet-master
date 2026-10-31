// Settings > General > Labs: one list of every early feature, each with a plain sentence about it.
import { labsOff, openSettings } from '../night/_area.mjs';

export const meta = {
  id: 'labs.index-lists-features',
  title: 'The Labs list names every early feature with a plain sentence',
  area: 'labs',
  tags: ['labs', 'smoke'],
};

const FEATURES = [
  ['Team', 'Tell teammates when your agent pushes, and see when theirs does.'],
  ['Ask bar', 'A small bar you open from anywhere in Windows to ask an agent a quick question.'],
  ['Simple mode', 'Fewer buttons and plain words, for people who do not write code.'],
  ['Live updates', 'Claude Code and Codex report when they start, ask and finish.'],
  ['Key switching', 'Move to your spare OpenCode key when one hits its rate limit.'],
  ['Missions', 'Agents work in a separate copy, turn by turn, until your checks pass.'],
  ['Assign an issue', 'Start an agent on a GitHub issue when someone on your list labels it.'],
  ['Night Shift', 'Agents work through your tasks overnight, in separate copies.'],
  ['Red to Green', 'The same agent fixes failed CI on its pull request, after you say yes.'],
  ['Potato Bot', 'A chat friend with its own brain that asks before it starts any helper.'],
];

export async function run(t) {
  await t.step('Open Settings > General');
  await openSettings(t, 'General');
  await t.expect.text('Each one is off until you turn it on, and may change.');

  await t.step('Every early feature is in the list');
  for (const [name, about] of FEATURES) {
    await t.expect.visible({ exact: name, within: 'css=.st-labs-list' });
    await t.expect.visible(`exact=${about}`);
  }
  await t.expect.eventually(async () => (await t.page.eval("document.querySelector('.st-labs-list').querySelectorAll('.st-labs-item').length")) === FEATURES.length, { message: `the Labs list holds ${FEATURES.length} features` });
}

export const setup = labsOff;

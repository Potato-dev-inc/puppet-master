// An agent that asks "Allow this command?" in the night is not answered: no yes, no no. The task needs a look.
import { openMorning, startOneTaskNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.approval-question-not-answered',
  title: 'A permission question in the night is left unanswered and the task needs a look',
  area: 'night',
  tags: ['night', 'safety'],
  edge: true,
  timeout: 90000,
};

const ASKS = `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "echo hi", on_allow = [{ do = "say", text = "allowed" }], on_deny = [{ do = "say", text = "denied" }] },
]
`;

export async function run(t) {
  await startOneTaskNight(t, { text: 'run a command', script: ASKS });
  await t.waitFor('exact=Open Morning', { timeout: 60000 });
  await t.expect.text('Needs you'); // the question reaches the sidebar; the page changes once more as it does
  await t.expect.visible('exact=Allow once');

  await t.step('The Morning page says the task needs a look');
  await openMorning(t);
  await t.expect.text('NEEDS A LOOK (1)');
  await t.expect.text('stopped at a question');
  await t.expect.text('Open its terminal to answer, then run the task again.');

  await t.step('The fake agent was never answered');
  const events = t.fake.events();
  t.expect.ok(events.some((e) => e.ev === 'approval_shown'), 'the question was shown');
  t.expect.equal(events.filter((e) => e.ev === 'approval_answer').length, 0, 'no answer was given');
  const shown = events.findIndex((e) => e.ev === 'approval_shown');
  const typed = events.slice(shown + 1).filter((e) => e.ev === 'key' && !['CtrlC', 'Escape'].includes(e.key));
  t.expect.equal(typed.length, 0, 'no answer key (y, n, a digit) was typed');
}

export const setup = nightSetup;

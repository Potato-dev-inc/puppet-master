// An agent that stops at a "do you trust this folder" screen is never answered by Night Shift: the task says it needs a
// look, the pane stays open on the question and nothing is typed into it.
import { openMorning, sclick, startOneTaskNight, nightSetup } from './_area.mjs';

export const meta = {
  id: 'night.trust-gate-needs-a-look',
  title: 'A trust question ends the try as Needs a look and is never answered',
  area: 'night',
  tags: ['night', 'safety'],
  edge: true,
  timeout: 90000,
};

const GATE = `
boot_ms = 100
inline = true
[[startup]]
gate = "trust"
after_ms = 200
[[turn]]
steps = [ { do = "think", ms = 300 }, { do = "say", text = "after trust" } ]
`;

const ROW = 'css=.ck-morning-task';

export async function run(t) {
  await startOneTaskNight(t, { text: 'try the gate', script: GATE });
  await t.waitFor('exact=Open Morning', { timeout: 60000 });
  await t.expect.text('Needs you'); // the question reaches the sidebar; the page changes once more as it does

  await t.step('Open the Morning page: the task needs a look and says why');
  await openMorning(t);
  await t.expect.text('0 of 1 done. 1 needs you.');
  await t.expect.text('NEEDS A LOOK (1)');
  await t.expect.text('Nimbus stopped at a question before it started.');
  await t.expect.text('Open its terminal to answer, then run the task again.');
  await t.expect.text('Nimbus stopped at a question before it started. It was not answered.');

  await t.step('Nothing was typed at the question');
  const events = t.fake.events();
  t.expect.ok(events.some((e) => e.ev === 'gate_shown'), 'the fake agent showed its trust question');
  const afterGate = events.slice(events.findIndex((e) => e.ev === 'gate_shown') + 1);
  t.expect.equal(afterGate.filter((e) => ['gate_answer', 'typed_at_gate', 'key'].includes(e.ev)).length, 0, 'no key or answer reached the question');

  await t.step('Its pane is still open, waiting on the question');
  const waiting = (await t.panes()).filter((p) => p.status === 'waiting_input');
  t.expect.equal(waiting.length, 1);

  await t.step('Keep on a task that needs a look asks with the reason');
  await sclick(t, 'exact=Keep', { within: ROW });
  await t.expect.text('Open its terminal to answer, then run the task again. Keep anyway?');
}

export const setup = nightSetup;

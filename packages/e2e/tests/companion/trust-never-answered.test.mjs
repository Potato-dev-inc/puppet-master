// A companion's agent that asks "Do you trust this folder?" is the person's to answer. The app shows the question and
// never presses a key itself.
import { noCompanions, invoke, draft, companionList, guarded } from './_area.mjs';

export const meta = {
  id: 'companion.trust-never-answered',
  title: "A trust question on the companion's pane waits for the person and is never answered by the app",
  area: 'companion',
  tags: ['security', 'startup-gate'],
  edge: true,
};

export const script = `
boot_ms = 100
inline = true
[[startup]]
gate = "trust"
`;

export const run = guarded(async (t) => {
  await noCompanions(t);
  t.fake.script(script);

  await t.step('Wake a companion whose agent asks to trust the folder');
  const made = await invoke(t, 'companion_create', { input: draft() });
  t.expect.ok(made.ok, 'the companion was made');
  const id = made.ok.id;

  await t.step('It says it needs you, with the question on its terminal');
  await t.expect.eventually(async () => (await companionList(t)).find((c) => c.id === id)?.state === 'needs_you', { timeout: 30000, message: 'expected the companion to need you' });
  await t.page.reload();
  await t.expect.visible('Needs you', { within: 'css=.ck-side' });
  await t.click(`exact=Mochi`, { within: 'css=.ck-side' });
  await t.expect.text('Needs you: Answer the agent');
  await t.expect.visible('exact=Open card');
  await t.expect.text('Do you trust the files in this folder?');
  await t.fake.waitEvent('gate_shown');

  await t.step('Several seconds pass: nothing was pressed for the person');
  await t.sleep(6000); // a negative needs time to be a proof: the keeper looks at the pane every second
  const events = t.fake.events().map((e) => e.ev);
  t.expect.ok(!events.includes('gate_answer'), `the fake agent was never answered (saw ${JSON.stringify(events)})`);
  t.expect.ok(!events.includes('key'), 'no key reached the agent');
  t.expect.equal((await companionList(t)).find((c) => c.id === id).state, 'needs_you', 'still waiting for the person');
  await t.expect.text('Do you trust the files in this folder?');
});

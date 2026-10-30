// The person answers the trust question from the Inbox card; then the companion goes on and is ready.
import { noCompanions, invoke, draft, companionList, guarded } from './_area.mjs';
import { script } from './trust-never-answered.test.mjs';

export const meta = {
  id: 'companion.trust-answered-by-person',
  title: "Answering the trust card in the Inbox lets the companion carry on",
  area: 'companion',
  tags: ['startup-gate'],
};

export const run = guarded(async (t) => {
  await noCompanions(t);
  t.fake.script(script);
  const made = await invoke(t, 'companion_create', { input: draft() });
  const id = made.ok.id;
  await t.expect.eventually(async () => (await companionList(t)).find((c) => c.id === id)?.state === 'needs_you', { timeout: 30000, message: 'expected the companion to need you' });

  await t.step('Open the card from the companion page');
  await t.page.reload();
  await t.click('exact=Mochi', { within: 'css=.ck-side' });
  await t.click('exact=Open card');
  await t.expect.visible('exact=Allow once');
  await t.expect.visible('exact=Deny');

  await t.step('Press Allow once');
  await t.click('exact=Allow once');

  await t.step('The agent was answered and the companion carries on');
  await t.fake.waitEvent('gate_answer');
  await t.expect.eventually(async () => (await companionList(t)).find((c) => c.id === id)?.state === 'idle', { timeout: 40000, message: 'expected the companion to be idle after the answer' });
});

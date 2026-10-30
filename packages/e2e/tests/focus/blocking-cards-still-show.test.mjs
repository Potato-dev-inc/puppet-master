// A card that blocks an agent is never hidden by Focus: the approval shows in the Inbox and answering it works.
import { focusReset, pickPane, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.blocking-cards-still-show',
  title: 'An agent waiting for an answer still shows in the Inbox during Focus',
  area: 'focus',
  tags: ['focus'],
  edge: true,
};

const SCRIPT = `
boot_ms = 100
inline = true
[[turn]]
on_prompt = "approve"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "echo hi", on_allow = [{ do = "say", text = "allowed and done" }], on_deny = [{ do = "say", text = "denied" }] },
]
`;

export async function run(t) {
  try {
    t.fake.script(SCRIPT);
    const pane = await t.spawnPane('nimbus');

    await t.step('Start Focus');
    await sclick(t, 'exact=45 min');
    await t.waitFor('css=[data-focus=on]');

    await t.step('The agent asks for permission');
    await pane.write('please approve the command');
    await t.fake.waitEvent('approval_shown');

    await t.step('The question is in the Inbox even though Focus is on');
    await t.expect.text('One thing needs you');
    await t.expect.visible('exact=Allow once');
    await t.expect.visible('exact=Deny');
    await t.expect.text('Things that need you still show.');

    await t.step('Answering works as usual');
    await t.click('exact=Allow once');
    await t.fake.waitEvent('approval_answer');
    await t.expect.text('Nothing needs you');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;

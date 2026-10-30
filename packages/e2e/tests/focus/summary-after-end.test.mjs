// Ending Focus with a question still waiting leaves one card, "While you focused", with counts; Got it dismisses it.
// An empty Focus (nothing happened) leaves no card (see focus.end-from-note).
import { focusReset, sclick } from '../night/_area.mjs';

export const meta = {
  id: 'focus.summary-after-end',
  title: 'Ending Focus leaves one While you focused card with counts and Got it clears it',
  area: 'focus',
  tags: ['focus'],
};

const ASKS = `
boot_ms = 100
inline = true
[[turn]]
on_prompt = "approve"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "bash", command = "echo hi", on_allow = [{ do = "say", text = "allowed" }], on_deny = [{ do = "say", text = "denied" }] },
]
`;

export async function run(t) {
  try {
    t.fake.script(ASKS);
    const pane = await t.spawnPane('nimbus');

    await t.step('Start Focus and let the agent ask a question that waits');
    await sclick(t, 'exact=45 min');
    await t.waitFor('css=[data-focus=on]');
    await pane.write('please approve the command');
    await t.fake.waitEvent('approval_shown');
    await t.expect.text('One thing needs you');

    await t.step('End Focus');
    await sclick(t, 'exact=End Focus');
    await t.waitFor('css=[data-focus=off]');

    await t.step('The summary card counts what was waiting');
    await t.expect.text('While you focused');
    await t.expect.text(/Focus ended after .*min: .*1 still waits\./);
    await t.expect.visible('exact=Got it');

    await t.step('Got it dismisses the card, and the question itself is still there');
    await sclick(t, 'exact=Got it');
    await t.expect.notVisible('exact=Got it');
    await t.expect.noText('While you focused');
    await t.expect.visible('exact=Allow once');
  } finally {
    await focusReset(t);
  }
}

export const setup = focusReset;

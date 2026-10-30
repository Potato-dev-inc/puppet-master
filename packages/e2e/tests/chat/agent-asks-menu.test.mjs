// An agent that stops with a numbered menu: Chat shows the "Answer the agent" card; clicking an option answers it.
import { openChat, sendChat } from '../_helpers.mjs';

export const meta = {
  id: 'chat.agent-asks-menu',
  title: 'A numbered menu from the agent becomes a card and a click answers it',
  area: 'chat',
  tags: ['smoke', 'questions'],
};

/** Exported so the Deny test plays the same agent. (TOML inline tables must stay on one line.) */
export const script = `
boot_ms = 100
inline = true
approvals = "menus"
[[turn]]
on_prompt = "run the command"
steps = [
  { do = "think", ms = 400 },
  { do = "approve", kind = "bash", command = "echo hi", on_allow = [{ do = "say", text = "the command ran" }], on_deny = [{ do = "say", text = "the command was refused" }] },
]
`;

export async function run(t) {
  t.fake.script(script);
  await t.spawnPane('nimbus');
  await openChat(t);

  await t.step('Ask the agent to run a command');
  await sendChat(t, 'please run the command');

  await t.step('The card asks the question with its options');
  await t.expect.text('Answer the agent');
  await t.expect.visible('exact=Allow once');
  await t.expect.visible('exact=Always allow in this project');
  await t.expect.visible('exact=Deny');
  await t.fake.waitEvent('approval_shown');

  await t.step('Click Allow once');
  await t.click('exact=Allow once');

  await t.step('The agent was answered and carries on');
  await t.fake.waitEvent('approval_answer');
  await t.expect.text('the command ran');
  await t.expect.notVisible('exact=Allow once');
}

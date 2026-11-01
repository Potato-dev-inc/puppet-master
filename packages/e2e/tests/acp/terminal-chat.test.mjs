// Chat (terminal), start to finish, so nobody has to open the terminal: Add chat > Chat (terminal) > start the agent as a
// chat (it opens on the Chat tab) > send a message > the reply shows > the agent asks to run something and a click on the
// card answers it > Stop (pane menu > Interrupt) > Remove chat > the row is back on Terminal.
// The agent is a fake program (a copy of Nimbus with no ACP table and a session-file reader): no real agent, no cost.
import path from 'node:path';
import { chatPanes, chooseOption, openSettings, reveal, MESSAGE_BOX } from './_area.mjs';
import { installAgent, SCRIBE_TABLES, sendChat, dismissAway } from '../chat/_area.mjs';
import { selectValue, LANE_SELECT as _unused } from './_robust.mjs';

export const meta = {
  id: 'acp.terminal-chat',
  title: 'Chat (terminal): add, start as a chat, message, answer a question with a click, stop, remove',
  area: 'acp',
  tags: ['acp', 'labs', 'terminal-chat', 'proof'],
  fresh: true,
  timeout: 300000,
};

const ID = 'tchat';
const LABS = 'aria=Chat sessions (ACP)';
const LANES = `select[aria-label="How ${ID} runs"]`;
const REMOVE_CSS = `button[aria-label="Remove chat for ${ID}"]`;
const SHEET = 'css=[data-testid=remove-chat-dialog]';
const shot = (t, name) => t.page.screenshot(path.join(t.outDir, `${name}.png`));
const beat = (t, ms = 700) => t.sleep(ms);
const labsOn = (t) => t.page.eval(`document.querySelector('[role=switch][aria-label="Chat sessions (ACP)"]')?.getAttribute('aria-checked')`);
const labsBackend = (t) => t.page.eval(`window.__TAURI_INTERNALS__.invoke('agent_lane_get').then((v) => String(v.labs_enabled), () => 'error')`);
const tcPanes = async (t) => (await t.panes()).filter((p) => p.lane === 'terminal_chat');

// Three kinds of message: a plain one, one that needs a permission, one that runs long (for Stop).
const SCRIPT = `
boot_ms = 100
inline = true
[[turn]]
on_prompt = "make the file"
steps = [
  { do = "think", ms = 300 },
  { do = "approve", kind = "create", path = "hello.txt", on_allow = [{ do = "write_file", path = "hello.txt", content = "hi" }, { do = "say", text = "file written" }], on_deny = [{ do = "say", text = "nothing written" }] },
]
[[turn]]
on_prompt = "longtask"
steps = [
  { do = "think", ms = 60000 },
  { do = "say", text = "finally done" },
]
[[turn]]
steps = [
  { do = "think", ms = 300 },
  { do = "say", text = "pong from tchat" },
]
`;

export async function run(t) {
  t.fake.script(SCRIPT);
  await installAgent(t, { id: ID, extra: SCRIBE_TABLES, withAcp: false });
  await dismissAway(t);

  await t.step('Turn on Chat sessions in General > Labs');
  await openSettings(t, 'General');
  await reveal(t, '[role=switch][aria-label="Chat sessions (ACP)"]');
  for (let attempt = 0; attempt < 4 && (await labsOn(t)) !== 'true'; attempt++) {
    await t.sleep(600);
    await t.click(LABS);
    await t.sleep(900);
  }
  await t.expect.eventually(async () => (await labsOn(t)) === 'true' && (await labsBackend(t)) === 'true', { message: 'the Labs switch is on and the app knows' });
  await beat(t);

  await t.step('Agents: Add chat for the agent that has none');
  await openSettings(t, 'Agents');
  const ADD = `button[aria-label="Add chat (ACP) for ${ID}"]`;
  await t.expect.eventually(async () => {
    await reveal(t, ADD);
    return t.page.has(`css=${ADD}`);
  }, { message: 'the Add chat button is listed for the agent' });
  await shot(t, '1-add-chat-button');
  await beat(t);
  await t.click(`css=${ADD}`);
  await t.waitFor('css=[data-testid=add-chat-dialog]');

  await t.step('Pick "Chat (terminal)"');
  // With other ways on offer there is a list to pick from; with only this one, its sentence is shown straight away.
  await t.expect.text('shows it as a chat', { timeout: 60000 });
  await t.expect.text('Nothing to download.');
  await beat(t);
  if (await t.page.has('css=[data-testid=add-chat-option-terminal_chat] input')) await t.click('css=[data-testid=add-chat-option-terminal_chat] input');
  await shot(t, '2-chat-terminal-picked');
  await beat(t);
  await t.click('exact=Add chat', { within: 'css=[data-testid=add-chat-dialog]' });
  await t.expect.eventually(async () => (await selectValue(t, LANES)) === 'terminal_chat', { timeout: 30000, message: 'How to run now says Chat (terminal)' });
  await t.expect.text(`Chat added for ${ID}.`);
  await shot(t, '3-added');
  await beat(t);

  await t.step('Start the agent as a chat from the + menu: it opens on the Chat tab');
  await t.click('aria=New agent');
  await t.waitFor(`css=[data-testid=lane-row-${ID}]`);
  await beat(t);
  await t.click(`exact=${ID}`, { within: `css=[data-testid=lane-row-${ID}]` });
  await t.expect.eventually(async () => (await tcPanes(t)).length === 1, { timeout: 30000, message: 'a Chat (terminal) pane is listed' });
  await t.waitFor(MESSAGE_BOX);
  await t.waitFor('css=[data-testid=pane-lane]');
  t.expect.equal((await chatPanes(t)).length, 0, 'it is not an ACP chat: a normal terminal pane');
  const pane = (await tcPanes(t))[0];
  t.expect.equal(pane.agent_type, ID);
  await shot(t, '4-chat-tab');
  await beat(t);

  await t.step('Send a message: the reply shows in the chat');
  await sendChat(t, 'ping');
  await t.expect.text('pong from tchat');
  await shot(t, '5-reply');
  await beat(t);

  await t.step('The agent asks to run something: the card is answered with a click');
  await sendChat(t, 'make the file');
  await t.expect.visible('exact=Allow once');
  await t.expect.visible('exact=Deny');
  t.log?.('answerable card present: ' + (await t.page.has('css=[data-answerable-question]')) + '; dock card text: ' + (await t.page.eval(`document.querySelector('.ck-chat-dock, .ck-dock')?.innerText?.slice(0,200)`)));
  await shot(t, '6-question-card');
  await beat(t, 1000);
  await t.click('exact=Allow once');
  await t.fake.waitEvent('approval_answer');
  await t.expect.text('file written');
  await t.expect.notVisible('exact=Allow once');
  await beat(t);

  await t.step('Stop: Interrupt ends a long turn');
  await sendChat(t, 'longtask');
  await t.expect.eventually(async () => (await t.panes()).find((p) => p.id === pane.id)?.status === 'running', { timeout: 20000, message: 'the long turn is running' });
  await beat(t);
  await t.page.rightClick('css=.ck-side .ck-row.is-active');
  await t.click('Interrupt');
  await t.fake.waitEvent('interrupted');
  await t.expect.eventually(async () => (await t.panes()).find((p) => p.id === pane.id)?.status === 'idle', { timeout: 20000, message: 'the agent is idle again' });
  await shot(t, '7-stopped');
  await beat(t);

  await t.step('Settings > Agents > Remove chat: the row goes back to Terminal');
  await openSettings(t, 'Agents');
  await t.expect.eventually(async () => {
    await reveal(t, REMOVE_CSS);
    return t.page.has(`css=${REMOVE_CSS}`);
  }, { message: 'the Remove chat button is listed' });
  await beat(t);
  await t.click(`aria=Remove chat for ${ID}`);
  await t.waitFor(SHEET);
  await shot(t, '8-remove-sheet');
  await beat(t);
  await t.click('exact=Remove', { within: SHEET });
  await t.expect.eventually(async () => !(await t.page.has(SHEET)), { timeout: 30000, message: 'the sheet is closed' });
  await t.expect.text(`Chat removed for ${ID}.`);
  await t.expect.eventually(async () => (await selectValue(t, LANES)) === 'terminal', { message: 'new sessions start in a terminal' });
  await shot(t, '9-back-to-terminal');
  await beat(t);
}

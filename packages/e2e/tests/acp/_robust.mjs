// Steps shared by the chat robustness tests (remove chat, crash and restart). Not a test.
import { chooseOption, chatPanes, calm, openSettings, goToPage, clickControl, revealControl, reveal, MESSAGE_BOX } from './_area.mjs';

const LABS = 'aria=Chat sessions (ACP)';
export const LANE_SELECT = 'select[aria-label="How Nimbus runs"]';
const labsOn = (t) => t.page.eval(`document.querySelector('[role=switch][aria-label="Chat sessions (ACP)"]')?.getAttribute('aria-checked')`);
const labsBackend = (t) => t.page.eval(`window.__TAURI_INTERNALS__.invoke('agent_lane_get').then((v) => String(v.labs_enabled), () => 'error')`);
export const selectValue = (t, selector) => t.page.eval(`document.querySelector(${JSON.stringify(selector)})?.value`);

/** Labs on, Nimbus set to Chat, a chat session started from the + menu. Returns the chat pane. */
export async function startNimbusChat(t) {
  await calm(t);
  await t.step('Turn on Chat sessions in General > Labs');
  await openSettings(t, 'General');
  await revealControl(t, LABS);
  for (let attempt = 0; attempt < 4 && (await labsOn(t)) !== 'true'; attempt++) {
    await t.sleep(600);
    await clickControl(t, LABS);
    await t.sleep(900);
  }
  await t.expect.eventually(async () => (await labsOn(t)) === 'true', { message: 'the Labs switch is on' });
  await t.expect.eventually(async () => (await labsBackend(t)) === 'true', { message: 'the app knows the switch is on' });

  await t.step('Nimbus runs as a Chat');
  await goToPage(t, 'Agents');
  await t.expect.eventually(() => t.page.eval(`!!document.querySelector(${JSON.stringify(LANE_SELECT)})`), { message: 'the How to run row for Nimbus' });
  await reveal(t, LANE_SELECT);
  await chooseOption(t, LANE_SELECT, 'Chat (ACP)');
  await t.expect.eventually(async () => (await selectValue(t, LANE_SELECT)) === 'acp', { message: 'the choice is saved' });

  await t.step('Start Nimbus from the + menu');
  await t.click('aria=New agent');
  await t.waitFor('css=[data-testid=lane-row-nimbus]');
  await t.click('exact=Nimbus', { within: 'css=[data-testid=lane-row-nimbus]' });
  await t.expect.eventually(async () => (await chatPanes(t)).length === 1, { timeout: 30000, message: 'a chat pane is listed' });
  await t.waitFor(MESSAGE_BOX);
  return (await chatPanes(t))[0];
}

export const chatState = (t, paneId) => t.page.eval(`window.__TAURI_INTERNALS__.invoke('acp_chat_state', { paneId: ${JSON.stringify(paneId)} })`);

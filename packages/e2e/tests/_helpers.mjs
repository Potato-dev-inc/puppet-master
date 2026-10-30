// Shared steps for tests. Keep them small and named after what a person does.

/** The sidebar rows of agent panes (not Inbox, not "Make a companion"). */
export const PANE_ROW = 'css=.ck-side .ck-row:not(.ck-companion-row):not(.ck-row--flat)';

/**
 * Select a pane in the sidebar: by its name ("Nimbus 2") or by position (0 = the first row). Rows can change order
 * while panes settle, so prefer the name when there is more than one pane.
 */
export async function selectPane(t, which = 0) {
  if (typeof which === 'string') await t.click(`exact=${which}`, { within: 'css=.ck-side' });
  else await t.click(PANE_ROW, { nth: which });
}

/** Select a pane in the sidebar and open its Chat tab. */
export async function openChat(t, n = 0) {
  await selectPane(t, n);
  await t.click('exact=Chat');
  await t.waitFor('aria=Message to the agent');
}

/** Type a message into the Chat composer and send it. */
export async function sendChat(t, text) {
  await t.page.fill('aria=Message to the agent', text);
  await t.click('exact=Send');
}

/** A script (pm-fake-tui TOML) for an agent that answers every message with `reply`. */
export const replyScript = (reply, thinkMs = 300) => `
boot_ms = 100
inline = true
[[turn]]
steps = [
  { do = "think", ms = ${thinkMs} },
  { do = "say", text = "${reply}" },
]
`;

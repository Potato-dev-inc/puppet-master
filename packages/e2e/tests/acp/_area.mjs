// Helpers for the chat (ACP) tests. Not a test.
export * from '../settings/_area.mjs';

/** Choose an option of a <select> by its visible text, with the events a person's pick makes. */
export async function chooseOption(t, selector, optionText) {
  await t.page.eval(`(() => {
    const sel = document.querySelector(${JSON.stringify(selector)});
    if (!sel) throw new Error('no select ' + ${JSON.stringify(selector)});
    const opt = [...sel.options].find((o) => o.text.trim() === ${JSON.stringify(optionText)});
    if (!opt) throw new Error('no option ' + ${JSON.stringify(optionText)} + ' in ' + [...sel.options].map((o) => o.text).join(', '));
    Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(sel, opt.value);
    sel.dispatchEvent(new Event('change', { bubbles: true }));
  })()`);
}

/** The chat panes (lane acp) the app lists. */
export async function chatPanes(t) {
  return (await t.panes()).filter((p) => p.lane === 'acp');
}

export const MESSAGE_BOX = 'aria=Message to the agent';

/** Type a message into the chat's box and press Send. */
export async function say(t, text) {
  await t.page.fill(MESSAGE_BOX, text);
  await t.click('exact=Send');
}

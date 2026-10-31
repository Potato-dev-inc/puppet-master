// Run by a fake agent's `run` step: start a pane through the app's local bridge, the way an agent's MCP tool or curl does,
// and (optionally) give it a first message once it is ready. Prints one line: `spawned <status> <pane id>`.
//   node spawn-helper.mjs <port> <agent> <cwd> [message words...]
const [port, agent, cwd, ...words] = process.argv.slice(2);
const base = `http://127.0.0.1:${port}`;
const call = async (method, path, body) => {
  const r = await fetch(base + path, { method, headers: { 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
  const text = await r.text();
  let json = null;
  try { json = JSON.parse(text); } catch { /* plain text */ }
  return { status: r.status, json, text };
};
const r = await call('POST', '/panes', { agent_type: agent, cwd, cols: 100, rows: 30 });
const id = r.json?.snapshot?.pane_id || r.json?.pane_id;
console.log(`spawned ${r.status} ${id || r.text.slice(0, 120)}`);
if (id && words.length) {
  for (let i = 0; i < 200; i++) {
    const list = (await call('GET', '/panes')).json || [];
    const buffer = (await call('GET', `/panes/${id}/buffer`)).json?.content ?? '';
    // ready = the app calls it idle AND the fake agent has drawn its input box (typing earlier is swallowed by the boot)
    if (list.find((p) => p.id === id)?.status === 'idle' && /Type a task/.test(buffer)) break;
    await new Promise((res) => setTimeout(res, 150));
  }
  const s = await call('POST', `/panes/${id}/input`, { text: words.join(' '), append_newline: true });
  console.log(`sent ${s.status}`);
}

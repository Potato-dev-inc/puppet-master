// A small Chrome DevTools Protocol client (WebView2 speaks CDP). Uses Node's built-in WebSocket.
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export async function listTargets(port) {
  const r = await fetch(`http://127.0.0.1:${port}/json/list`, { signal: AbortSignal.timeout(3000) });
  return r.json();
}

export async function connect(wsUrl) {
  const ws = new WebSocket(wsUrl);
  await new Promise((res, rej) => { ws.addEventListener('open', res, { once: true }); ws.addEventListener('error', () => rej(new Error('CDP websocket failed')), { once: true }); });
  let id = 0;
  const pending = new Map();
  const listeners = new Set();
  let closed = false;
  ws.addEventListener('message', (ev) => {
    const m = JSON.parse(String(ev.data));
    if (m.id !== undefined && pending.has(m.id)) {
      const p = pending.get(m.id); pending.delete(m.id);
      m.error ? p.reject(new Error(`${p.method}: ${m.error.message}`)) : p.resolve(m.result);
    } else for (const l of listeners) { try { l(m); } catch { /* a listener must not break the socket */ } }
  });
  ws.addEventListener('close', () => { closed = true; for (const p of pending.values()) p.reject(new Error('CDP connection closed')); pending.clear(); });
  const send = (method, params = {}) => new Promise((resolve, reject) => {
    if (closed) return reject(new Error('CDP connection closed'));
    const i = ++id; pending.set(i, { resolve, reject, method }); ws.send(JSON.stringify({ id: i, method, params }));
  });
  /** Evaluate an expression in the page (REPL semantics, awaits promises, returns JSON-able values). */
  const evalJs = async (expression) => {
    const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
    if (r.exceptionDetails) throw new Error('page error: ' + (r.exceptionDetails.exception?.description || r.exceptionDetails.text || '').slice(0, 400));
    return r.result.value;
  };
  return {
    send, evalJs,
    on: (l) => { listeners.add(l); return () => listeners.delete(l); },
    get closed() { return closed; },
    close: () => { try { ws.close(); } catch { /* already closed */ } },
  };
}

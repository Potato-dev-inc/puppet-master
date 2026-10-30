// The app's local HTTP bridge, called the way the desktop UI calls it (the profile's secret in a header).
export class BridgeClient {
  constructor(port, secret) {
    this.port = port;
    this.secret = secret;
    this.base = `http://127.0.0.1:${port}`;
  }

  async req(method, p, body) {
    const r = await fetch(this.base + p, {
      method,
      headers: { 'content-type': 'application/json', 'x-pm-local-secret': this.secret },
      body: body === undefined ? undefined : JSON.stringify(body),
      signal: AbortSignal.timeout(30000),
    });
    const text = await r.text();
    let json = null;
    try { json = text ? JSON.parse(text) : null; } catch { /* plain text answer */ }
    return { status: r.status, ok: r.ok, body: json ?? text };
  }

  get(p) { return this.req('GET', p); }
  post(p, body = {}) { return this.req('POST', p, body); }
  del(p) { return this.req('DELETE', p); }
}

// The harness has no backend: anything that would talk to a local bridge must fail fast instead of
// hanging. A pending request also stops headless Chrome's virtual time from ever advancing.
export function blockNetwork(): void {
  const realFetch = window.fetch.bind(window);
  window.fetch = (input, init) => {
    const url = new URL(
      typeof input === 'string' ? input : input instanceof URL ? input.href : input.url,
      window.location.href,
    );
    if (url.origin !== window.location.origin) {
      console.warn(`[harness] blocked fetch ${url.href}`);
      return Promise.reject(new TypeError('harness: network disabled'));
    }
    return realFetch(input, init);
  };
  class DeadEventSource extends EventTarget {
    readyState = 2;
    url: string;
    constructor(url: string | URL) {
      super();
      this.url = String(url);
      window.setTimeout(() => this.dispatchEvent(new Event('error')), 0);
    }
    close() {}
  }
  window.EventSource = DeadEventSource as unknown as typeof EventSource;
  window.WebSocket = class {
    constructor(url: string | URL) {
      throw new DOMException(`harness: WebSocket disabled (${String(url)})`, 'SecurityError');
    }
  } as unknown as typeof WebSocket;
}

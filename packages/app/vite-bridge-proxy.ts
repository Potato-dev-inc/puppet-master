import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import type { ProxyOptions } from 'vite';

const PORT_FILE = resolve(__dirname, '../../puppet-master.bridge.port');

function readBridgeTarget(): string {
  try {
    if (existsSync(PORT_FILE)) {
      const raw = readFileSync(PORT_FILE, 'utf-8').trim();
      if (raw.includes(':')) {
        const [host, port] = raw.split(':');
        return `http://${host || '127.0.0.1'}:${port}`;
      }
      return `http://127.0.0.1:${raw}`;
    }
  } catch {
    /* fall through */
  }
  return 'http://127.0.0.1:17321';
}

/** Proxy `/bridge/*` to the desktop app's embedded HTTP bridge (dynamic port from port file). */
export const bridgeProxyConfig: Record<string, ProxyOptions> = {
  '/bridge': {
    target: 'http://127.0.0.1:17321',
    changeOrigin: true,
    rewrite: (path) => path.replace(/^\/bridge/, '') || '/',
    router: () => readBridgeTarget(),
  },
};

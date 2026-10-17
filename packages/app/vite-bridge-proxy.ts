import { existsSync, readFileSync } from 'node:fs';
import type { IncomingMessage } from 'node:http';
import { homedir } from 'node:os';
import { join } from 'node:path';
import type { ProxyOptions } from 'vite';

// Where the desktop app writes its bridge port file. Mirrors src-tauri/src/app_paths.rs
// (`bridge_port_file()`): the file lives in the app's data folder, named per profile.
const APP_ID = 'com.puppetmaster.app';
const PORT_FILE_NAME = 'puppet-master.bridge.port';
const PORT_FILE_ENV = 'PUPPET_MASTER_BRIDGE_PORT_FILE';
const PROFILE_ENV = 'PUPPET_MASTER_PROFILE';
const DEFAULT_TARGET = 'http://127.0.0.1:17321';

/** Same rule as `app_paths::profile_from`: `[a-z0-9-]`, 1..=32, not leading `-`, not `default`. */
function profileFrom(raw: string | undefined): string | null {
  const name = (raw ?? '').trim().toLowerCase();
  const valid = name.length > 0 && name.length <= 32 && name !== 'default' && /^[a-z0-9-]+$/.test(name) && !name.startsWith('-');
  return valid ? name : null;
}

/**
 * The port file the running app writes: `PUPPET_MASTER_BRIDGE_PORT_FILE` when set, else
 * `<app data>/com.puppetmaster.app[.profile-<name>]/puppet-master.bridge.port`.
 * (Nothing writes a port file in the repo root any more, so there is no fallback to one.)
 */
export function resolveBridgePortFile(
  env: Record<string, string | undefined> = process.env,
  platform: NodeJS.Platform = process.platform,
  home: string = homedir(),
): string {
  const override = env[PORT_FILE_ENV]?.trim();
  if (override) return override;
  let root: string;
  switch (platform) {
    case 'win32':
      root = env.APPDATA || home;
      break;
    case 'darwin':
      root = join(home, 'Library', 'Application Support');
      break;
    default:
      root = join(home, '.local', 'share');
  }
  const profile = profileFrom(env[PROFILE_ENV]);
  return join(root, profile ? `${APP_ID}.profile-${profile}` : APP_ID, PORT_FILE_NAME);
}

/** `http://host:port` from the port file contents (`host:port` or a bare port), or null if unusable. */
function parseTarget(raw: string): string | null {
  const text = raw.trim();
  if (!text) return null;
  const [host, port] = text.includes(':') ? [text.slice(0, text.lastIndexOf(':')), text.slice(text.lastIndexOf(':') + 1)] : ['', text];
  if (!/^\d{1,5}$/.test(port.trim())) return null;
  return `http://${host.trim() || '127.0.0.1'}:${port.trim()}`;
}

/** The bridge the app is serving right now. Read on every call: the app picks a new port each launch. */
export function readBridgeTarget(): string {
  const file = resolveBridgePortFile();
  try {
    if (existsSync(file)) {
      const target = parseTarget(readFileSync(file, 'utf-8'));
      if (target) return target;
    }
  } catch {
    /* fall through */
  }
  return DEFAULT_TARGET;
}

type ProxyServer = Parameters<NonNullable<ProxyOptions['configure']>>[0];

const CONNECTION_ERRORS = new Set(['ECONNREFUSED', 'ECONNRESET', 'ENOTFOUND', 'EHOSTUNREACH', 'ENETUNREACH', 'ETIMEDOUT', 'EAI_AGAIN']);

/**
 * Vite builds its proxy on node `http-proxy`, which fixes `target` when the dev server starts (it has
 * no `router` option; that is http-proxy-middleware). So each `web`/`ws` call gets the current
 * target from the port file, and a dead bridge answers a clear 502 instead of Vite's empty 500.
 */
function configureProxy(proxy: ProxyServer): void {
  const targets = new WeakMap<IncomingMessage, string>();

  const web = proxy.web.bind(proxy) as (...args: unknown[]) => unknown;
  (proxy as unknown as { web: unknown }).web = (req: IncomingMessage, res: unknown, opts?: object, cb?: unknown) => {
    const target = readBridgeTarget();
    targets.set(req, target);
    // http-proxy counts trailing arguments, so an `undefined` callback must not be passed along.
    const options = { ...(opts ?? {}), target };
    return cb ? web(req, res, options, cb) : web(req, res, options);
  };

  const ws = proxy.ws.bind(proxy) as (...args: unknown[]) => unknown;
  (proxy as unknown as { ws: unknown }).ws = (req: IncomingMessage, socket: unknown, head: unknown, opts?: unknown, cb?: unknown) => {
    const callback = typeof opts === 'function' ? opts : cb;
    const options = typeof opts === 'object' && opts ? opts : {};
    const target = readBridgeTarget();
    targets.set(req, target);
    const merged = { ...options, target };
    return callback ? ws(req, socket, head, merged, callback) : ws(req, socket, head, merged);
  };

  // Registered before Vite's own handler, which then sees headersSent and leaves the reply alone.
  proxy.on('error', (err, req, res) => {
    const reply = res as unknown as { headersSent?: boolean; writableEnded?: boolean; writeHead?: Function; end?: Function };
    if (typeof reply.writeHead !== 'function' || reply.headersSent || reply.writableEnded) return;
    const target = targets.get(req as IncomingMessage) ?? readBridgeTarget();
    const port = target.slice(target.lastIndexOf(':') + 1);
    const code = (err as NodeJS.ErrnoException).code ?? '';
    const unreachable = CONNECTION_ERRORS.has(code) || err.name === 'AggregateError';
    const body = JSON.stringify(
      unreachable
        ? {
            code: 'BRIDGE_UNREACHABLE',
            // No file path here: dev runs can sit behind a public tunnel, and the path holds the Windows user name.
            message: `The Puppet Master app's bridge is not running on port ${port}. Start or restart the app.`,
          }
        : { code: 'BRIDGE_PROXY_ERROR', message: `The Puppet Master bridge proxy failed: ${err.message}` },
    );
    reply.writeHead(502, { 'Content-Type': 'application/json', 'Cache-Control': 'no-store' });
    reply.end?.(body);
  });

  proxy.on('proxyReq', (proxyReq) => {
    proxyReq.setHeader('X-PM-Proxied', '1');
  });
  proxy.on('proxyReqWs', (proxyReq) => {
    proxyReq.setHeader('X-PM-Proxied', '1');
  });
}

/** Proxy `/bridge/*` to the desktop app's embedded HTTP bridge (port read from its port file per request). */
export const bridgeProxyConfig: Record<string, ProxyOptions> = {
  '/bridge': {
    // Overridden on every request by `configureProxy`; this is only the value before the app has run.
    target: DEFAULT_TARGET,
    changeOrigin: true,
    ws: true,
    rewrite: (path) => path.replace(/^\/bridge/, '') || '/',
    configure: configureProxy,
  },
};

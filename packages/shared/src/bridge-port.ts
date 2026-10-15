import { readFile, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { dirname, join } from 'node:path';
import {
  BRIDGE_LOCAL_SECRET_FILE,
  BRIDGE_LOCAL_SECRET_HEADER,
  BRIDGE_PORT_FILE_ENV,
  DEFAULT_BRIDGE_PORT_FILE,
} from './protocol.js';

const APP_ID = 'com.puppetmaster.app';

function defaultAppDataBridgePortFile(): string {
  const home = homedir();
  switch (process.platform) {
    case 'win32':
      return join(process.env.APPDATA ?? home, APP_ID, DEFAULT_BRIDGE_PORT_FILE);
    case 'darwin':
      return join(home, 'Library', 'Application Support', APP_ID, DEFAULT_BRIDGE_PORT_FILE);
    default:
      return join(home, '.local', 'share', APP_ID, DEFAULT_BRIDGE_PORT_FILE);
  }
}

function parseBridgePort(raw: string): { port: number; host: string } {
  const trimmed = raw.trim();
  if (trimmed.includes(':')) {
    const [host, portStr] = trimmed.split(':');
    return { host: host || '127.0.0.1', port: Number(portStr) };
  }
  return { host: '127.0.0.1', port: Number(trimmed) };
}

/** Expands `%VAR%` so MCP client `env` blocks can reference `%APPDATA%`. Unknown vars stay as-is. */
export function expandEnvVars(input: string, env: NodeJS.ProcessEnv = process.env): string {
  return input.replace(/%([^%]+)%/g, (match, name: string) => env[name] ?? match);
}

function bridgePortCandidates(filePath?: string): string[] {
  if (filePath) return [filePath];
  const envPath = process.env[BRIDGE_PORT_FILE_ENV];
  if (envPath) return [expandEnvVars(envPath)];
  return [DEFAULT_BRIDGE_PORT_FILE, defaultAppDataBridgePortFile()];
}

export async function readAllBridgePorts(
  filePath?: string,
): Promise<Array<{ host: string; port: number }>> {
  const found: Array<{ host: string; port: number }> = [];
  const seen = new Set<string>();
  for (const fp of [...new Set(bridgePortCandidates(filePath))]) {
    try {
      const parsed = parseBridgePort(await readFile(fp, 'utf-8'));
      if (!Number.isFinite(parsed.port) || parsed.port <= 0) continue;
      const key = `${parsed.host}:${parsed.port}`;
      if (seen.has(key)) continue;
      seen.add(key);
      found.push(parsed);
    } catch {
      /* try next candidate */
    }
  }
  return found;
}

export async function readBridgePort(filePath?: string): Promise<{ port: number; host: string }> {
  const found = await readAllBridgePorts(filePath);
  if (found[0]) return found[0];
  const candidates = [...new Set(bridgePortCandidates(filePath))];
  throw new Error(
    `Puppet Master bridge port file not found (tried: ${candidates.map((p) => `"${p}"`).join(', ')}). ` +
    `Start Puppet Master first (\`npx puppet-master\`).`
  );
}

/**
 * The `X-PM-Local-Secret` header for a request to the local bridge, or `{}` when there is no secret
 * file (an older app). The secret is read from beside the same port file `readBridgePort` uses, on
 * every call, so a restarted app's new secret is picked up. Same lookup as the Rust MCP shim
 * (`mcp_shim/endpoint.rs::local_secret_header`). The bridge only logs a missing secret
 * for now; callers never fail because of it.
 */
export async function readLocalSecretHeaders(filePath?: string): Promise<Record<string, string>> {
  for (const candidate of [...new Set(bridgePortCandidates(filePath))]) {
    try {
      if (!(await stat(candidate)).isFile()) continue;
    } catch {
      continue;
    }
    try {
      const secret = (await readFile(join(dirname(candidate), BRIDGE_LOCAL_SECRET_FILE), 'utf-8')).trim();
      return /^[0-9a-fA-F]+$/.test(secret) ? { [BRIDGE_LOCAL_SECRET_HEADER]: secret } : {};
    } catch {
      return {};
    }
  }
  return {};
}

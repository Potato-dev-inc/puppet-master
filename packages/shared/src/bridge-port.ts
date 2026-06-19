import { readFile } from 'node:fs/promises';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { BRIDGE_PORT_FILE_ENV, DEFAULT_BRIDGE_PORT_FILE } from './protocol.js';

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

function bridgePortCandidates(filePath?: string): string[] {
  if (filePath) return [filePath];
  const envPath = process.env[BRIDGE_PORT_FILE_ENV];
  if (envPath) return [envPath];
  return [DEFAULT_BRIDGE_PORT_FILE, defaultAppDataBridgePortFile()];
}

export async function readBridgePort(filePath?: string): Promise<{ port: number; host: string }> {
  const candidates = [...new Set(bridgePortCandidates(filePath))];
  let lastPath = candidates[candidates.length - 1] ?? DEFAULT_BRIDGE_PORT_FILE;
  for (const fp of candidates) {
    lastPath = fp;
    try {
      return parseBridgePort(await readFile(fp, 'utf-8'));
    } catch {
      /* try next candidate */
    }
  }
  throw new Error(
    `Puppet Master bridge port file not found (tried: ${candidates.map((p) => `"${p}"`).join(', ')}). ` +
    `Start Puppet Master first (\`npx puppet-master\`).`
  );
}
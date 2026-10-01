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
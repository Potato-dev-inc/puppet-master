import { readFile } from 'node:fs/promises';
import { BRIDGE_PORT_FILE_ENV, DEFAULT_BRIDGE_PORT_FILE } from './protocol.js';

export async function readBridgePort(filePath?: string): Promise<{ port: number; host: string }> {
  const fp = filePath ?? process.env[BRIDGE_PORT_FILE_ENV] ?? DEFAULT_BRIDGE_PORT_FILE;
  let raw: string;
  try {
    raw = await readFile(fp, 'utf-8');
  } catch (err) {
    throw new Error(
      `Puppet Master bridge port file not found at "${fp}". ` +
      `Start Puppet Master first (\`npx puppet-master\`).`
    );
  }
  const trimmed = raw.trim();
  if (trimmed.includes(':')) {
    const [host, portStr] = trimmed.split(':');
    return { host: host || '127.0.0.1', port: Number(portStr) };
  }
  return { host: '127.0.0.1', port: Number(trimmed) };
}
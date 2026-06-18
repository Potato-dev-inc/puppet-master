#!/usr/bin/env node
/**
 * Zero-config dev tunnel (Cloudflare Quick Tunnel) + optional ngrok override.
 * Like Expo `--tunnel`: no account or token required by default.
 */
import { writeFile, unlink } from 'node:fs/promises';
import { existsSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '..');
export const DEV_INFO_FILE = join(repoRoot, 'puppet-master.dev.json');

export function isTunnelEnabled() {
  return process.env.PUPPET_MASTER_TUNNEL !== '0';
}

function preferNgrok() {
  return Boolean(process.env.NGROK_AUTHTOKEN?.trim());
}

/**
 * @param {number} port
 * @returns {Promise<{ url: string, provider: string, close: () => Promise<void> } | null>}
 */
export async function startDevTunnel(port) {
  if (!isTunnelEnabled()) return null;

  const localUrl = `http://127.0.0.1:${port}`;

  if (preferNgrok()) {
    return startNgrokTunnel(localUrl);
  }
  return startCloudflaredTunnel(localUrl);
}

async function startCloudflaredTunnel(localUrl) {
  const { Tunnel } = await import('cloudflared');
  const tunnel = Tunnel.quick(localUrl);

  const url = await new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      reject(new Error('cloudflared tunnel timed out waiting for public URL'));
    }, 45_000);
    tunnel.once('url', (publicUrl) => {
      clearTimeout(timeout);
      resolve(publicUrl);
    });
    tunnel.once('error', (err) => {
      clearTimeout(timeout);
      reject(err);
    });
  });

  return {
    url,
    provider: 'cloudflared',
    close: async () => {
      tunnel.stop();
    },
  };
}

async function startNgrokTunnel(localUrl) {
  const ngrok = await import('@ngrok/ngrok');
  const listener = await ngrok.forward({
    addr: localUrl,
    authtoken_from_env: true,
  });

  const url = listener.url();
  if (!url) {
    throw new Error('ngrok started but returned no public URL');
  }

  return {
    url,
    provider: 'ngrok',
    close: async () => {
      await listener.close();
    },
  };
}

/**
 * @param {{ localUrl: string, tunnelUrl?: string | null, tunnelProvider?: string | null, bridgeUrl?: string | null }} info
 */
export async function writeDevInfo(info) {
  const tunnelUrl = info.tunnelUrl ?? null;
  const payload = {
    localUrl: info.localUrl,
    pwaUrl: info.localUrl,
    tunnelUrl,
    tunnelProvider: info.tunnelProvider ?? null,
    /** @deprecated use tunnelUrl */
    ngrokUrl: tunnelUrl,
    bridgeProxyUrl: tunnelUrl ? `${tunnelUrl}/bridge` : `${info.localUrl}/bridge`,
    bridgeDirectUrl: info.bridgeUrl ?? null,
    updatedAt: Date.now(),
  };
  await writeFile(DEV_INFO_FILE, `${JSON.stringify(payload, null, 2)}\n`, 'utf-8');
  return payload;
}

export async function clearDevInfo() {
  if (existsSync(DEV_INFO_FILE)) {
    await unlink(DEV_INFO_FILE);
  }
}

export function tunnelDisabledHint() {
  return 'Tunnel disabled (PUPPET_MASTER_TUNNEL=0).';
}

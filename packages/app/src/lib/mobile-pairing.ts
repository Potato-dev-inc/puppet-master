import {
  PairingSessionInfoSchema,
  PairResponseSchema,
  pairResponseMessage,
  parsePairingQrPayload,
  type PairingQrPayload,
  type PairingSessionInfo,
  type PairResponse,
} from '@puppet-master/shared';
import { type BridgeLocation, LS_BRIDGE_URL, resolveBridgeBaseUrl } from './bridge-url';
import { mergeBridgeHeaders } from './mobile-pairing-auth';

export const LS_MOBILE_PAIRING = 'pm-mobile-pairing-v1';

export interface MobilePairingCredentials {
  bridgeUrl: string;
  deviceId: string;
  deviceToken: string;
  serverPublicKeyB64: string;
  devicePrivateKeyJwk: JsonWebKey;
  devicePublicKeyB64: string;
  deviceName: string;
  pairedAt: number;
}

function b64(bytes: ArrayBuffer | Uint8Array): string {
  const view = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  let binary = '';
  for (const byte of view) binary += String.fromCharCode(byte);
  return btoa(binary);
}

function b64Decode(value: string): Uint8Array<ArrayBuffer> {
  const binary = atob(value);
  const out = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) out[i] = binary.charCodeAt(i);
  return out;
}

export async function generateDeviceKeyPair(): Promise<{
  publicKeyB64: string;
  privateKeyJwk: JsonWebKey;
}> {
  const keyPair = await crypto.subtle.generateKey({ name: 'Ed25519' }, true, ['sign', 'verify']);
  const rawPublic = await crypto.subtle.exportKey('raw', keyPair.publicKey);
  const privateKeyJwk = await crypto.subtle.exportKey('jwk', keyPair.privateKey);
  return { publicKeyB64: b64(rawPublic), privateKeyJwk };
}

async function importServerPublicKey(publicKeyB64: string): Promise<CryptoKey> {
  return crypto.subtle.importKey(
    'raw',
    b64Decode(publicKeyB64),
    { name: 'Ed25519' },
    false,
    ['verify'],
  );
}

export async function verifyPairResponse(
  response: PairResponse,
  devicePublicKeyB64: string,
): Promise<boolean> {
  const key = await importServerPublicKey(response.server_public_key);
  const message = pairResponseMessage(
    response.device_id,
    response.device_token,
    devicePublicKeyB64,
  );
  const signature = b64Decode(response.server_signature);
  return crypto.subtle.verify('Ed25519', key, signature, new TextEncoder().encode(message));
}

export function loadMobilePairingCredentials(): MobilePairingCredentials | null {
  try {
    const raw = localStorage.getItem(LS_MOBILE_PAIRING);
    if (!raw) return null;
    return JSON.parse(raw) as MobilePairingCredentials;
  } catch {
    return null;
  }
}

export function saveMobilePairingCredentials(creds: MobilePairingCredentials): void {
  localStorage.setItem(LS_MOBILE_PAIRING, JSON.stringify(creds));
}

export function clearMobilePairingCredentials(): void {
  localStorage.removeItem(LS_MOBILE_PAIRING);
}

const PAIRING_FETCH_MS = 12_000;

function fetchWithTimeout(url: string, init: RequestInit = {}, ms = PAIRING_FETCH_MS): Promise<Response> {
  const controller = new AbortController();
  const timer = window.setTimeout(() => controller.abort(), ms);
  const { signal: outerSignal, ...rest } = init;
  if (outerSignal) {
    outerSignal.addEventListener('abort', () => controller.abort(), { once: true });
  }
  return fetch(url, { ...rest, signal: controller.signal }).finally(() => {
    window.clearTimeout(timer);
  });
}

function abortError(err: unknown): boolean {
  return err instanceof DOMException && err.name === 'AbortError';
}

async function readFetchError(res: Response): Promise<string> {
  const text = await res.text();
  return parseBridgeError(text) || `Request failed (${res.status})`;
}

export async function claimPairing(
  bridgeUrl: string,
  payload: PairingQrPayload,
  deviceName: string,
): Promise<MobilePairingCredentials> {
  const { publicKeyB64, privateKeyJwk } = await generateDeviceKeyPair();
  const base = bridgeUrl.replace(/\/$/, '');
  let res: Response;
  try {
    res = await fetchWithTimeout(`${base}/pair`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        pairing_code: payload.c,
        device_name: deviceName,
        device_public_key: publicKeyB64,
      }),
    });
  } catch (err) {
    if (abortError(err)) throw new Error('Pairing timed out — is the desktop app running?');
    throw err;
  }
  if (!res.ok) {
    throw new Error(await readFetchError(res));
  }
  const body = PairResponseSchema.parse(await res.json());
  if (body.server_public_key !== payload.pk) {
    throw new Error('Server public key mismatch — possible MITM');
  }
  const verified = await verifyPairResponse(body, publicKeyB64);
  if (!verified) {
    throw new Error('Invalid server signature');
  }
  const creds: MobilePairingCredentials = {
    bridgeUrl: payload.u.replace(/\/$/, ''),
    deviceId: body.device_id,
    deviceToken: body.device_token,
    serverPublicKeyB64: body.server_public_key,
    devicePrivateKeyJwk: privateKeyJwk,
    devicePublicKeyB64: publicKeyB64,
    deviceName,
    pairedAt: Date.now(),
  };
  saveMobilePairingCredentials(creds);
  return creds;
}

export async function fetchPairingSessionInfo(
  bridgeUrl: string,
  pairingCode: string,
): Promise<PairingSessionInfo> {
  const base = bridgeUrl.replace(/\/$/, '');
  let res: Response;
  try {
    res = await fetchWithTimeout(
      `${base}/pair/session/${encodeURIComponent(pairingCode)}`,
    );
  } catch (err) {
    if (abortError(err)) {
      throw new Error('Could not reach desktop bridge — open /bridge/health on this URL to test');
    }
    throw err;
  }
  if (!res.ok) {
    throw new Error(await readFetchError(res));
  }
  return PairingSessionInfoSchema.parse(await res.json());
}

function parseBridgeError(text: string): string {
  const trimmed = text.trim();
  if (!trimmed) return '';
  try {
    const body = JSON.parse(trimmed) as { error?: unknown };
    if (typeof body.error === 'string') return body.error;
  } catch {
    /* plain text */
  }
  return trimmed;
}

async function bridgeHealthOk(bridgeUrl: string): Promise<boolean> {
  try {
    const res = await fetchWithTimeout(
      `${bridgeUrl.replace(/\/$/, '')}/health`,
      { headers: mergeBridgeHeaders({}) },
      4000,
    );
    return res.ok;
  } catch {
    return false;
  }
}

async function bridgeTokenValid(bridgeUrl: string): Promise<boolean> {
  try {
    const res = await fetchWithTimeout(
      `${bridgeUrl.replace(/\/$/, '')}/settings`,
      { headers: mergeBridgeHeaders({}) },
      4000,
    );
    return res.ok;
  } catch {
    return false;
  }
}

/** Reuse saved credentials when invite link was already consumed (e.g. React StrictMode). */
export async function reconnectIfAlreadyPaired(bridgeUrl: string): Promise<MobilePairingCredentials | null> {
  const creds = loadMobilePairingCredentials();
  if (!creds) return null;
  const target = bridgeUrl.replace(/\/$/, '');
  const saved = creds.bridgeUrl.replace(/\/$/, '');
  if (saved !== target) return null;
  if (!(await bridgeHealthOk(target))) return null;
  if (!(await bridgeTokenValid(target))) return null;
  return creds;
}

export async function pairFromInviteCode(
  pairingCode: string,
  deviceName: string,
  location: BridgeLocation = window.location,
  storedBridgeUrl: string | null = localStorage.getItem(LS_BRIDGE_URL),
  onStage?: (label: string) => void,
): Promise<MobilePairingCredentials> {
  const bridgeUrl = resolveBridgeBaseUrl(storedBridgeUrl, location);
  onStage?.('Checking bridge…');
  const existing = await reconnectIfAlreadyPaired(bridgeUrl);
  if (existing) return existing;

  onStage?.('Loading pairing session…');
  const info = await fetchPairingSessionInfo(bridgeUrl, pairingCode);
  if (info.expires_at * 1000 < Date.now()) {
    throw new Error('Pairing link expired — refresh QR on desktop');
  }
  const connectBridge = bridgeUrl.replace(/\/$/, '');
  const payload: PairingQrPayload = {
    v: 1,
    u: connectBridge,
    pk: info.server_public_key,
    c: info.pairing_code,
    e: info.expires_at,
  };
  onStage?.('Signing in…');
  try {
    return await claimPairing(connectBridge, payload, deviceName);
  } catch (err) {
    const after = await reconnectIfAlreadyPaired(bridgeUrl);
    if (after) return after;
    throw err;
  }
}

export async function pairFromQrJson(
  qrJson: string,
  deviceName: string,
): Promise<MobilePairingCredentials> {
  const payload = parsePairingQrPayload(qrJson);
  if (payload.e * 1000 < Date.now()) {
    throw new Error('Pairing code expired — refresh QR on desktop');
  }
  return claimPairing(payload.u, payload, deviceName);
}

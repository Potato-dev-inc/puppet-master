import { z } from 'zod';

/** HTTP header set by the Vite `/bridge` proxy so the embedded bridge requires pairing tokens. */
export const PM_PROXIED_HEADER = 'X-PM-Proxied';

/** Bearer token issued after a successful `/pair` exchange. */
export const PM_AUTH_HEADER = 'Authorization';

export const PairingQrPayloadSchema = z.object({
  v: z.literal(1),
  /** PWA origin or `/bridge` base URL the phone should use. */
  u: z.string().url(),
  /** Desktop server Ed25519 public key (32 bytes, base64). */
  pk: z.string().min(1),
  /** Short-lived pairing code shown on desktop. */
  c: z.string().min(4).max(32),
  /** Unix seconds when the pairing code expires. */
  e: z.number().int().positive(),
});
export type PairingQrPayload = z.infer<typeof PairingQrPayloadSchema>;

export const PairRequestSchema = z.object({
  pairing_code: z.string().min(4).max(32),
  device_name: z.string().min(1).max(120),
  /** Device Ed25519 public key (32 bytes, base64). */
  device_public_key: z.string().min(1),
});
export type PairRequest = z.infer<typeof PairRequestSchema>;

export const PairResponseSchema = z.object({
  device_id: z.string().uuid(),
  device_token: z.string().min(16),
  server_public_key: z.string().min(1),
  /** Ed25519 signature over `device_id|device_token|device_public_key`. */
  server_signature: z.string().min(1),
});
export type PairResponse = z.infer<typeof PairResponseSchema>;

export const PairedDeviceInfoSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  public_key: z.string(),
  paired_at: z.number().int(),
});
export type PairedDeviceInfo = z.infer<typeof PairedDeviceInfoSchema>;

export const PairingSessionSchema = z.object({
  pairing_code: z.string(),
  expires_at: z.number().int(),
  server_public_key: z.string(),
  bridge_url: z.string(),
  qr_payload: PairingQrPayloadSchema,
});
export type PairingSession = z.infer<typeof PairingSessionSchema>;

/** Public lookup for an active desktop pairing session (GET /pair/session/:code). */
export const PairingSessionInfoSchema = z.object({
  pairing_code: z.string(),
  expires_at: z.number().int().positive(),
  server_public_key: z.string().min(1),
  bridge_url: z.string().url(),
});
export type PairingSessionInfo = z.infer<typeof PairingSessionInfoSchema>;

export function buildPairingInviteUrl(pwaOrigin: string, pairingCode: string): string {
  const origin = pwaOrigin.trim().replace(/\/+$/, '');
  const code = pairingCode.trim();
  return `${origin}/pair/${encodeURIComponent(code)}`;
}

/** Extract pairing code from `/pair/CODE` invite paths. */
export function parsePairingInvitePath(pathname: string): string | null {
  const trimmed = pathname.trim();
  const match = trimmed.match(/^\/pair\/([A-Za-z0-9]+)\/?$/);
  return match ? decodeURIComponent(match[1]).toUpperCase() : null;
}

export function encodePairingQrPayload(payload: PairingQrPayload): string {
  return JSON.stringify(payload);
}

/** QR content: prefer invite URL; fall back to legacy JSON payload. */
export function encodePairingQrContent(
  payload: PairingQrPayload,
  pwaOrigin?: string | null,
): string {
  if (pwaOrigin?.trim()) {
    return buildPairingInviteUrl(pwaOrigin, payload.c);
  }
  return encodePairingQrPayload(payload);
}

export function parsePairingQrPayload(raw: string): PairingQrPayload {
  const trimmed = raw.trim();
  const json = trimmed.startsWith('{') ? trimmed : decodeURIComponent(trimmed);
  return PairingQrPayloadSchema.parse(JSON.parse(json));
}

/** Extract pairing code from a scanned invite URL or `/pair/CODE` path. */
export function parsePairingInviteFromQr(raw: string): string | null {
  const trimmed = raw.trim();
  if (trimmed.startsWith('http://') || trimmed.startsWith('https://')) {
    try {
      return parsePairingInvitePath(new URL(trimmed).pathname);
    } catch {
      return null;
    }
  }
  if (trimmed.startsWith('/pair/')) return parsePairingInvitePath(trimmed);
  return null;
}

export function pairResponseMessage(
  deviceId: string,
  deviceToken: string,
  devicePublicKeyB64: string,
): string {
  return `${deviceId}|${deviceToken}|${devicePublicKeyB64}`;
}

export function bridgeRequiresPairingToken(headers: Record<string, string | undefined>): boolean {
  return headers[PM_PROXIED_HEADER] === '1' || headers[PM_PROXIED_HEADER.toLowerCase()] === '1';
}

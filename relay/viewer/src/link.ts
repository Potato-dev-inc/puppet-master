// The secret part of the link. The browser never sends the fragment to the relay, so the room id, the
// read token and the content key stay on this page. Shape: #<room_id>.<read_token>.<content_key>

export interface RoomLink {
  /** 26 base32 characters (SPEC-n32 3.2). The relay checks the exact alphabet. */
  room: string;
  /** 16 random bytes, base64url. Sent as a header, never in the path. */
  readToken: string;
  /** 32 random bytes, base64url. Only used to open frames. */
  contentKey: string;
}

const LINK = /^#([0-9A-Za-z]{26})\.([A-Za-z0-9_-]{22})\.([A-Za-z0-9_-]{43})$/;

/** Reads the fragment of the page address. Returns null when it is missing or malformed. */
export function parseLink(hash: string): RoomLink | null {
  const match = LINK.exec(hash);
  if (!match) {
    return null;
  }
  return { room: match[1] ?? '', readToken: match[2] ?? '', contentKey: match[3] ?? '' };
}

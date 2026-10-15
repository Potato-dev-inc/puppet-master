/**
 * The key a client address is counted under, the same rule as relay/server's `address_key`: an IPv4
 * address as is, an IPv6 address by its /64 (one household or server gets a whole /64, so counting
 * single IPv6 addresses would let one client rotate around a limit). IPv4-mapped IPv6 addresses
 * count as the IPv4 address. Anything unreadable (or no header at all) is `"unknown"`.
 */

const HEXTET = /^[0-9a-f]{1,4}$/i;

function parseV4(text: string): number[] | undefined {
  const parts = text.split(".");
  if (parts.length !== 4) return undefined;
  const out: number[] = [];
  for (const part of parts) {
    if (!/^\d{1,3}$/.test(part)) return undefined;
    const n = Number(part);
    if (n > 255) return undefined;
    out.push(n);
  }
  return out;
}

/** Eight 16-bit groups, or undefined. Handles `::` and a trailing IPv4 part. */
function parseV6(text: string): number[] | undefined {
  if (!text.includes(":")) return undefined;
  const halves = text.split("::");
  if (halves.length > 2) return undefined;
  const side = (s: string): number[] | undefined => {
    if (s === "") return [];
    const groups: number[] = [];
    const parts = s.split(":");
    for (let i = 0; i < parts.length; i += 1) {
      const part = parts[i];
      if (i === parts.length - 1 && part.includes(".")) {
        const v4 = parseV4(part);
        if (!v4) return undefined;
        groups.push((v4[0] << 8) | v4[1], (v4[2] << 8) | v4[3]);
      } else if (HEXTET.test(part)) {
        groups.push(parseInt(part, 16));
      } else {
        return undefined;
      }
    }
    return groups;
  };
  const head = side(halves[0]);
  if (!head) return undefined;
  if (halves.length === 1) return head.length === 8 ? head : undefined;
  const tail = side(halves[1]);
  if (!tail || head.length + tail.length > 7) return undefined;
  return [...head, ...new Array<number>(8 - head.length - tail.length).fill(0), ...tail];
}

export function addressKeyOf(raw: string | null | undefined): string {
  if (!raw) return "unknown";
  let text = raw.trim();
  if (text.startsWith("[") && text.endsWith("]")) text = text.slice(1, -1);
  const zone = text.indexOf("%");
  if (zone >= 0) text = text.slice(0, zone);
  const v4 = parseV4(text);
  if (v4) return v4.join(".");
  const groups = parseV6(text);
  if (!groups) return "unknown";
  if (groups.slice(0, 5).every((g) => g === 0) && groups[5] === 0xffff) {
    return [groups[6] >> 8, groups[6] & 255, groups[7] >> 8, groups[7] & 255].join(".");
  }
  return `${groups.slice(0, 4).map((g) => g.toString(16)).join(":")}::/64`;
}

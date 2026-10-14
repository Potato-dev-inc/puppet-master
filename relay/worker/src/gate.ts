import { DurableObject } from "cloudflare:workers";
import type { Env } from "./env";

/**
 * One per client address (the object's name is a hash of the address, so no address is stored):
 * counts the unauthenticated calls (team registrations, join requests) in a window, like the
 * `ip:` counter of relay/server. Counted exactly, because a Durable Object is a single place.
 */
export class IpGate extends DurableObject<Env> {
  /** Count one more call in a window of `spanMs`; false when over `max`. */
  async hit(spanMs: number, max: number): Promise<boolean> {
    const now = Date.now();
    const saved = await this.ctx.storage.get<{ start: number; count: number }>("window");
    let start = now;
    let count = 1;
    if (saved && now - saved.start < spanMs) {
      start = saved.start;
      count = saved.count + 1;
    }
    await this.ctx.storage.put("window", { start, count });
    // Forget the address when the window is over.
    await this.ctx.storage.setAlarm(start + spanMs + 1_000);
    return count <= max;
  }

  async alarm(): Promise<void> {
    await this.ctx.storage.deleteAll();
  }
}

/** SHA-256 of the address as hex (the Durable Object's name). */
export async function addressKey(address: string): Promise<string> {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(`pm-relay-ip-v1\0${address}`));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

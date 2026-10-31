import { createServer } from "node:http";
import type { IncomingMessage, Server, ServerResponse } from "node:http";
import type { AddressInfo } from "node:net";

/**
 * A stand-in for the app's bridge, on port 0 (never a fixed port). It holds
 * the inbox, records every posted event, and checks the token header.
 */
export interface MockBridge {
  url: string;
  token: string;
  /** every event posted so far, in order */
  events: Array<Record<string, unknown>>;
  /** the posts as they came: each body and the headers that carried it */
  posts: Array<{ body: unknown; headers: IncomingMessage["headers"] }>;
  /** the inbox queries seen (path and query) */
  polls: string[];
  push(...items: Array<Record<string, unknown>>): void;
  /** answer every call with this status until cleared (null) */
  failWith(status: number | null): void;
  /** answer only event posts with this status (the inbox keeps working) */
  failPosts(status: number | null): void;
  waitFor<T = Record<string, unknown>>(
    pred: (e: Record<string, unknown>) => boolean,
    ms?: number,
  ): Promise<T>;
  close(): Promise<void>;
}

export async function startMockBridge(token = "tok-test"): Promise<MockBridge> {
  const queue: Array<Record<string, unknown>> = [];
  let taken = 0;
  let fail: number | null = null;
  let failPost: number | null = null;
  const events: Array<Record<string, unknown>> = [];
  const posts: MockBridge["posts"] = [];
  const polls: string[] = [];

  const server: Server = createServer((req: IncomingMessage, res: ServerResponse) => {
    const url = new URL(req.url ?? "/", "http://127.0.0.1");
    if (fail !== null) {
      res.writeHead(fail).end();
      return;
    }
    if (req.headers["x-pm-potato-token"] !== token) {
      res.writeHead(404).end();
      return;
    }

    if (req.method === "GET" && url.pathname === "/potato/brain/inbox") {
      polls.push(`${url.pathname}${url.search}`);
      const wait = Math.min(Number(url.searchParams.get("wait_ms") ?? 0), 400);
      const started = Date.now();
      const tick = (): void => {
        if (queue.length > taken || Date.now() - started >= wait) {
          const items = queue.slice(taken);
          taken = queue.length;
          res.writeHead(200, { "Content-Type": "application/json" });
          res.end(JSON.stringify({ v: 1, cursor: taken, items }));
          return;
        }
        setTimeout(tick, 15);
      };
      tick();
      return;
    }

    if (req.method === "POST" && url.pathname === "/potato/brain/events") {
      if (failPost !== null) {
        req.resume();
        res.writeHead(failPost).end();
        return;
      }
      const chunks: Buffer[] = [];
      req.on("data", (c: Buffer) => chunks.push(c));
      req.on("end", () => {
        const text = Buffer.concat(chunks).toString("utf8");
        let body: unknown = null;
        try {
          body = JSON.parse(text);
        } catch {
          res.writeHead(400).end();
          return;
        }
        posts.push({ body, headers: req.headers });
        const list = (body as { events?: unknown }).events;
        if (Array.isArray(list)) {
          for (const e of list) events.push(e as Record<string, unknown>);
        }
        res.writeHead(200, { "Content-Type": "application/json" }).end("{}");
      });
      return;
    }

    res.writeHead(404).end();
  });

  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  const port = (server.address() as AddressInfo).port;

  const bridge: MockBridge = {
    url: `http://127.0.0.1:${port}`,
    token,
    events,
    posts,
    polls,
    push: (...items) => {
      for (const item of items) queue.push({ v: 1, ...item });
    },
    failWith: (status) => {
      fail = status;
    },
    failPosts: (status) => {
      failPost = status;
    },
    async waitFor(pred, ms = 8000) {
      const until = Date.now() + ms;
      for (;;) {
        const hit = events.find(pred);
        if (hit) return hit as never;
        if (Date.now() > until) {
          const seen = events.map((e) => String(e["type"])).join(",");
          throw new Error(`timed out waiting for an event; saw: [${seen}]`);
        }
        await new Promise((r) => setTimeout(r, 15));
      }
    },
    close: () =>
      new Promise<void>((resolve) => {
        server.closeAllConnections();
        server.close(() => resolve());
      }),
  };
  return bridge;
}

/** A tiny OpenAI-style endpoint that always answers with this status and body. */
export async function startFakeModel(
  status: number,
  body: string,
): Promise<{ url: string; hits: number; close: () => Promise<void> }> {
  const state = { url: "", hits: 0, close: async () => {} };
  const server = createServer((req, res) => {
    req.resume();
    req.on("end", () => {
      state.hits++;
      res.writeHead(status, { "Content-Type": "application/json" }).end(body);
    });
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  state.url = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
  state.close = () =>
    new Promise<void>((resolve) => {
      server.closeAllConnections();
      server.close(() => resolve());
    });
  return state;
}

# Live preview

**Status:** Works today. Tested only by automated tests. The test screens were written to match each tool's printed format. They were not captured from live runs.

## What it is

When an agent starts a web page on your own PC (a "dev server", for example Vite on port 5173), Puppet Master notices the address in the terminal. It shows a **Preview** button on the agent. One click opens the page in your normal browser.

## Why it matters

People who build with AI mostly care about the page, not the terminal. They want to see what the agent made. This feature puts "Open the page" one click away. Developers save the step of copying the address from the terminal.

## How to use it

1. Ask an agent to start a project that serves a page (for example, "run the dev server").
2. When the terminal prints an address such as `http://localhost:5173/`, within about a second you see:
   - a **Preview** chip in the agent's header (hover it to see "Has a preview · localhost:5173. Opens in your browser."),
   - a small globe icon on the agent's row in the sidebar.
3. Click **Preview**. The page opens in your default browser.
4. If the agent started more than one server, press Ctrl+K and look for **Open preview: Claude Code :5173**. Each server has its own line, up to three. The chip opens the newest one.
5. On a paired phone, a note under the tab bar shows "Preview" with the address and "Open on your PC". The phone cannot open the page itself.
6. In Simple mode there is a separate **Preview** tab that shows the page the agent made in its copy (see [Simple mode](07-simple-mode.md)). It has **Open in browser**.

## How it works (simple)

About once a second, and only when the screen changed, Puppet Master looks at the last 24 non-empty lines of each agent's screen. It looks for `http://` or `https://` addresses on `localhost`, `127.0.0.1`, `[::1]` or `0.0.0.0`, and for private network addresses such as "Network: http://192.168.1.9:5173" (which it keeps as a second address). `0.0.0.0` is shown as `localhost`.

It keeps one entry per port, newest first, at most three per agent. Addresses on the public internet are ignored on purpose, so a docs link printed in the output is never offered as a preview. When the agent's process ends, its previews are removed. A click uses the same "open a link" command as the rest of the app, which accepts only `http` and `https`.

## Limits and what is not done yet

- A preview stays offered until the agent's process ends, even if the server has stopped.
- A banner that scrolls away within one second of output can be missed. An address that wraps over two terminal lines is cut at the end of the line.
- Only the newest address gets the chip. The other two appear in Ctrl+K.
- **The phone does not get a proxy, by design.** Sending a dev server through your tunnel would publish whatever the agent runs (often with no login and with debug pages) to anyone who learns the tunnel address. A safe remote preview would need its own short-lived, signed proxy. That is not built.
- It was not tried on a real phone.
- It does not tell you if the page is healthy. It only knows the address was printed.

## For developers

- Rust: `src-tauri/src/pane_preview.rs` (`detect_urls`, `observe`, `retain_alive`). It is fed by `pane_activity::tick_with`, not by the terminal reader.
- Channels: Tauri command `pane_previews`, event `pty://preview`, SSE `pane-preview`, read-only route `GET /previews` (`bridge/routes/previews.rs`). The MCP tool catalog is not changed.
- UI: `lib/pane-preview.ts` (`usePanePreviews`, `openPreview`), the chip in `components/cockpit/PaneView.tsx`, the globe in `Sidebar.tsx`, palette items in `Palette.tsx`, `styles/live-preview.css`.
- Phone: `components/mobile/PreviewNote.tsx`.

## Related

- [The cockpit](01-cockpit.md)
- [Command palette and ask bar](06-command-palette-and-ask-bar.md)
- [Simple mode](07-simple-mode.md)

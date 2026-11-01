# Sandbox screen viewer

This folder holds the live screen viewer. It shows a sandbox desktop inside an iframe, and can let a person take over the mouse and keyboard.

## Files

- `embed.html` - the page the iframe loads. It starts the noVNC client, shows the desktop, and reports its status to the parent page. It is view-only unless the URL asks for control.
- `clipboard-bridge.js` - copy and paste between the host clipboard and the remote desktop.
- `mobile-keyboard.js` - the touch keyboard, the Esc / Tab / Ctrl / arrow key bar, and the touch trackpad for phones.
- `novnc/` - not in this folder yet. noVNC will be added later under its own MPL-2.0 licence. Do not copy noVNC files here by hand. The page reads noVNC from this one folder, set by the `NOVNC_BASE` constant in `embed.html`.
- `PORTED.md` - what was changed from the original Rakazo files.

## How the parent page talks to it

1. Load the page in an iframe with URL parameters:
   - `?control=1` lets the person type and click. Without it the page is view-only. Adding `view_only=1` forces view-only even when `control=1` is set.
   - `?path=websockify` sets the websocket path. Only letters, digits, `_`, `.`, `/` and `-` are accepted. Anything else falls back to `websockify`.
2. Allow the parent's origin. The serving layer fills in this tag in `embed.html`:
   `<meta name="pm-sbx-parent-origins" content="https://app.example.test">`
   It takes exact origins separated by commas. A `*` is refused. An empty value means no status is sent at all.
3. Listen for status messages. The page posts to the parent only, with this shape:
   `{ type: "pm-sbx-screen-status", status: "...", reason: "..." }`
   - `status` is one of `loading`, `connecting`, `connected`, `disconnected`, `failed`.
   - `reason` is only sent with `failed` or `disconnected`. It is one of `load`, `timeout`, `security`, `closed`, `error`. No server text is ever sent.
   - If the page is not `connected` within 15 seconds, it reports `failed` with `timeout`. Show an error and a Retry button. Retry means reloading the iframe.
4. Check incoming messages on the parent side: the `event.origin` you expect, `event.source === iframe.contentWindow`, and the `type` above. The page accepts no messages from the parent.
5. Native apps can take over the keyboard. Set `window.__pmSbxNativeKeys = true` before the page's scripts run. Then call `window.pmSbxComputerKeyboard.run(command)` with a command such as `{ type: "text", text: "hi" }`, `{ type: "backspace", count: 1 }`, or `{ type: "key", name: "Escape", control: false }`.

## Serving rules (outside this folder)

- Send `Cache-Control: no-transform` so no proxy changes the page.
- Allow framing only from the parent origins you trust.
- Do not offer H.264 video. The page already hides `VideoDecoder`.

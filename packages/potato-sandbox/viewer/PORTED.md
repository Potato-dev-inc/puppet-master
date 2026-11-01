# PORTED.md - viewer

Source: Rakazo clone, commit f48fa5c7, Apache-2.0. Port map: D:\pm-research\RAKAZO-PORT-MAP.md.

| our file | their file | what changed | lines kept / removed |
|---|---|---|---|
| `embed.html` | `infra/sandboxes/computer/embed.html` | Title and page name changed to "Sandbox computer". noVNC is now loaded with dynamic `import()` from the single constant `NOVNC_BASE = "./novnc/"` (was static imports from `./core/...`). View-only is now the default; control needs `?control=1` (`view_only=1` forces view-only). Status is posted to the parent as `pm-sbx-screen-status` (loading, connecting, connected, disconnected, failed) only to origins in the `pm-sbx-parent-origins` meta allowlist; `*` and malformed entries are refused. A 15 s stall timer reports `failed` with `timeout`. A `VideoDecoder` removal runs before noVNC loads (from the proxy shim in their docs, added here so it also applies on our serving path). `path` is limited to safe characters. `data-rakazo-screen-error` renamed to `data-pm-sbx-screen-error`; `window.__rakazoNativeKeys` renamed to `window.__pmSbxNativeKeys`. The securityfailure and disconnect text from noVNC is no longer forwarded. The VNC password is read from the URL fragment `#vnc=` (removed from the address bar at once) and sent on `credentialsrequired`; no password means `failed`/`security` (added in the integration pass; Rakazo's server had no password). The old `view_only` default (control on) is gone. | 192 kept, 47 removed (239 original lines). The `<style>` block and `<body>` markup are byte-identical to the original. The script block was rewritten. Final file 367 lines. |
| `clipboard-bridge.js` | `infra/sandboxes/computer/clipboard-bridge.js` | Header comment added. No code changes. | 205 kept, 0 removed. Final file 211 lines (6 header lines added). |
| `mobile-keyboard.js` | `infra/sandboxes/computer/mobile-keyboard.js` | Header comment added. Window hooks renamed: `__rakazoNativeKeys` to `__pmSbxNativeKeys` (2 places) and `rakazoComputerKeyboard` to `pmSbxComputerKeyboard` (2 places, plus one comment). | 491 kept, 4 lines edited, 0 removed. Final file 502 lines (7 header lines added). |

## Not ported

- `clipboard-bridge.d.ts` and `mobile-keyboard.d.ts` - type declarations, not in the brief. Skipped; can be added later if the TypeScript side needs them.
- noVNC core (`core/...`) - not a Rakazo file. Vendored unmodified in `novnc/` from the npm package `@novnc/novnc` 1.7.0 (MPL-2.0; see `novnc/README.md`).
- Serving-side pieces (web proxy shim, `Cache-Control: no-transform`, the 15 s Retry UI in the app) - these live outside this folder. The embed page covers the reporting and the `VideoDecoder` shim only.

## Checks run

- `node --check` on `clipboard-bridge.js` and `mobile-keyboard.js`: pass.
- `node --check` on the module script extracted from `embed.html`: pass.
- Smoke test of the parent-origin allowlist (8 cases) and the `path` sanitising (6 cases), run against the functions extracted from `embed.html`: all passed.
- Grep for `rakazo` outside header blocks: none, except the rename list in the `mobile-keyboard.js` header, which is allowed there.

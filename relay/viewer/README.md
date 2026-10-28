# Live Room viewer

The read-only page the relay serves at `/r/`. Plain TypeScript, no framework, no runtime dependencies.

- `src/`: `viewer.ts` (boot and poll loop), `api.ts` (three GET calls), `crypto.ts` (WebCrypto AES-256-GCM),
  `render.ts` (pure model), `board.ts` (DOM, `textContent` only), `link.ts` (the `#` fragment), `viewer.html`, `viewer.css`.
- `dist/` is committed and served as is. After any change in `src/` run `npm run build` (esbuild, deterministic).
- Link: `https://<relay>/r/#<room>.<read_token>.<content_key>`. The key never leaves the page; the relay sees only ciphertext.
- Calls send the read token in `x-pm-room-read` and a random seat id per page load in `x-pm-room-seat`.
- The page has no inputs, no storage, no cookies and no third-party loads. The relay's CSP enforces the same.
- `test/vector.json` is the shared vector from the app's sealer (`packages/app/src-tauri/src/live_room/seal.rs`). The app's test checks it, and this page's test opens it.
- Tests: `npm test` (vitest, jsdom for the DOM parts). Types: `npm run typecheck`.

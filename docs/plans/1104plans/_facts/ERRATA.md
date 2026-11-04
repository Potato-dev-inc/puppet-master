# Errata: plan lines that disagree with the code

Found by checking the Potato Bot plans (10 to 15) against the code on 2026-11-04. The code wins. A builder reads this file BEFORE the plan, and `potato-bot-answers.md` for the real names. Line numbers are in the plan files.

| Plan | Line | The plan says | The code has |
|---|---|---|---|
| 11 Discord | many | `Outside(o)` and `Origin::Phone` exist | Neither exists. `Origin` enums are in `acp/registry.rs:167` and `cockpit.rs:13` and have no `Phone`. A slice that needs `Phone` must ADD it (say so in the slice) |
| 11 Discord | 967-968 | calls Host methods | Those methods are not on `trait Host`. Add them to the trait in the same slice, or use what the trait has (see answers file) |
| 12 Groups | 1234, 1344 | Labs key `potato_groups_enabled` | The Labs key that exists is `potato_bot_enabled`. A NEW key is allowed only if the slice also edits both `DEV_LABS_KEYS` lists |
| 13 Sandbox | 419 | `src/pm.ts` | The brain entry is `brain.mjs` |
| 13 Sandbox | 626 | provider method `complete` | The method is `chat` |
| 14 Phone/Telegram | 46 | `RateBook` | Does not exist. Create it in the slice or use the existing limiter named in the answers file |
| 14 Phone/Telegram | 48 | `allowed_mentions` | Not in the code. It is a Discord API field: the slice must add it to the request it builds |
| 14 Phone/Telegram | many | `Origin::Phone` | Does not exist (see Plan 11) |
| 15 Personas | 524, 1070 | `updated_ms` on open-work items | Not a field of that type. `updated_ms` exists on conversation records (`acp/record.rs:78`) only |

Also: 25 of 146 open facts could not be found (marked `NOT FOUND` in `potato-bot-answers.md`). A builder must not guess those: stop and ask.

## Spaces plans (20 to 27), checked 2026-11-04

| Plan | The plan says | The code has |
|---|---|---|
| 21 | the `spaces.ts` snippet copies a settings-hook import from `desk.ts` | `desk.ts` has no such import. Read the answers file (F-8, U-1) for the real one |
| 22 N-3 | `pairing_commands::ready` reads the settings JSON | It reads a static flag (`LABS`), set by `apply_settings` |
| 26 | tests run with `npx vitest run packages/potato-brain/...` | Potato Brain uses `node --test test/*.test.ts`. There is no vitest there |
| 26 | the brain channel message has a `kind` field | The code uses `type` |
| 26 P-6 | `render::clean` exists | It does not exist. The slice must create it or drop it |
| 27 line 80 | `apply_body` and `ingest.rs` already exist | Neither exists, and there is no `spaces/` folder yet. They are new files in an earlier slice |
| 22 W-1, W-2 | the BIP-39 English list has a known hash and licence | No copy of the list is in the repo. The builder must download it from the official BIP-39 repo, record the SHA-256 it computes, and note the licence (MIT) |
| 27 F-1 | the app has a Downloads-folder helper | None found. Add one, or ask the person with a file picker |
| 27 B-2 | the paid relay covers a Spaces mailbox | Undecided. Treat the mailbox as off |

## Relay plans (30 to 35), checked 2026-11-04

| Plan | Line | The plan says | The code has |
|---|---|---|---|
| 31 | 1504 | the server has a `--max-db-bytes` flag | `relay/server/src/main.rs` has no such flag. The slice must add it |
| 32 | 1694 | `KeyStore::set_relay_token` | No such function. The slice must add it |

Relay facts that stay unknown (marked NOT FOUND in `relay-answers.md`): how Railway shows client addresses behind its proxy (NF-8, decides if the join limit can be shared by strangers), Cloudflare free-plan limits and token labels (NF-11, NF-31, NF-35), the `wrangler deploy --dry-run` output (NF-12), and where TypeScript declares Labs keys (NF-19). Do not guess these.

Only the contradictions above were checked. Other references to existing names in plans 30 to 35 were NOT swept.

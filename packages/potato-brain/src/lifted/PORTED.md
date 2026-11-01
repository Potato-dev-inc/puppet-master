# Ported from Rakazo

Source: Rakazo, commit f48fa5c7 (Apache-2.0). Each file carries the header from `LIFT-RULES.md`.
Paths in the table are relative to the Rakazo `packages/` folder. Line counts come from a diff against the source file: "kept" is source lines that survive unchanged in the port, "removed or changed" is source lines that do not.

| our file | their file | what changed | lines kept/removed |
|---|---|---|---|
| `src/lifted/context-pruning.ts` | `adapters/src/context-pruning.ts` | `@earendil-works/pi-ai` `Message` replaced by local `PruneMessage` types (text and image parts only). Added `dropRepeatedFrameImages`: a screenshot whose frame equals the previous image-bearing screenshot's frame loses its image bytes. `pruneForRebuild` runs it last. | about 116 kept / 24 removed or changed (of 140) |
| `src/lifted/tool-loop.ts` | `adapters/src/tool-loop.ts`, plus `trackToolCallStreak` and `ToolCallStreak` from `core/src/events.ts` | `@rakazo/core` imports removed; `trackToolCallStreak` moved in from events.ts. Thresholds unchanged (6 identical, 25 similar). The two thresholds are now exported. | about 37 kept / 8 removed or changed (of 45), plus 18 lines of trackToolCallStreak |
| `src/lifted/silent-reply.ts` | `adapters/src/silent-reply.ts` | `MessageBlock` from `@rakazo/contracts` replaced by a generic `ReplyBlock` (`kind`, optional `text`, other fields kept). Functions are generic over the block type. Logic unchanged. | about 120 kept / 31 removed or changed (of 151) |
| `src/lifted/prompt-data.ts` | `core/src/prompt-data.ts` | Doc comment added. Function unchanged: it escapes `&`, `<`, `>` only. Rakazo has no fence wrapper, so none is added here. | 3 kept / 0 removed (of 3) |
| `src/lifted/redact-secrets.ts` | `core/src/events.ts` (`protectedValues`, `redactedRanges`, `redactSecrets` only) | Extended: each secret is also matched in standard base64, url-safe base64 (padded and unpadded forms), percent-encoded, form-encoded (`+` for space) and lower-case percent-escape form. `redactSecretsAfter`, `containsSecret`, the streaming redactor and the rest of events.ts are not ported. | about 54 of the 77 source lines kept; rest not ported |
| `src/lifted/screen-lease.ts` | `core/src/screen-lease.ts` | SPEC comment added at the top for the Rust mirror. Fence parsing accepts only ASCII digits within the safe-integer range (Rakazo's `Number()` accepted `""`, `" 3"`, `"3e0"`). Added `screenLeaseHeld` (expiry). The take and release rules are unchanged. | 28 kept / 2 removed or changed (of 30) |
| `src/lifted/tool-schemas.ts` | `adapters/src/builtin-tools.ts` (computer_observe, computer_act, browser_navigate, browser_snapshot, browser_act), `adapters/src/browser-tools.ts` (`BROWSER_ACT_KINDS`) | Plain JSON-schema objects in OpenAI function-tool form (`{type:"function", function:{name, description, parameters}}`), no zod. `computer_act` adds `minItems: 1` and `maxItems: 24` on `actions` (Rakazo enforces 24 in code). Descriptions and property shapes are unchanged. `fill_card` field list written inline. | about 125 source lines of schema, reproduced as objects; not line-diffable |

## Tests

| our file | their file | what changed |
|---|---|---|
| `test/lifted-context-pruning.test.ts` | `adapters/src/context-pruning.test.ts` | Ported from vitest to node:test; all cases kept; cases for `dropRepeatedFrameImages` added |
| `test/lifted-tool-loop.test.ts` | `adapters/src/tool-loop.test.ts`, and the `trackToolCallStreak` cases in `core/src/events.test.ts` | Ported; tool names made generic |
| `test/lifted-silent-reply.test.ts` | none upstream (the module has no own test file) | Written from the doc comments |
| `test/lifted-prompt-data.test.ts` | `core/src/prompt-data.test.ts` | Ported; one case added |
| `test/lifted-redact-secrets.test.ts` | `core/src/events.test.ts` (the `redactSecrets` cases) | Ported; base64, url-safe base64 and percent-encoding cases added |
| `test/lifted-screen-lease.test.ts` | `core/src/screen-lease.test.ts` | Ported; fence-parsing, equal-fence, and expiry cases added |
| `test/lifted-tool-schemas.test.ts` | none upstream | Written: names, kinds, the 24 cap, JSON round trip |

## Decisions to review

1. **Expiry.** Rakazo's screen-lease logic has no expiry; it lives in the database layer (`expiresAt` on execution leases). The brief asked for it, so `screenLeaseHeld` is a one-line helper. An expired lease holds nothing, but its fence is still the floor for a take (SPEC rule 4).
2. **Identical frames.** The "identical frame omits image bytes" rule is Rakazo's `computer-tools.ts` (`observationToolResult`), not `context-pruning.ts`. The port adds `dropRepeatedFrameImages` to match it at transcript rebuild. Rakazo's own runtime path already omits the bytes for an unchanged frame, so this is a safety net.
3. **"Last 3".** `pruneStalePageStateContext` keeps the last 3 page-state results that carry at least 1,000 characters of text (Rakazo's rule). Image-only screenshots are kept by `pruneComputerScreenshotContext`, which keeps the last 2 by default and honors a model image limit. Rakazo does it this way; the brief's "last 3 screenshot/page results" read as one count is not what Rakazo does.
4. **Silent reply.** `NO_RESPONSE` handling is unchanged. The rule the brief names (exact token means post nothing) is `stripNoResponseReply`.
5. **Fence strictness.** The fence must be digits only. A malformed id parses as the whole string with fence 0, the same as Rakazo's fallback for a missing fence.

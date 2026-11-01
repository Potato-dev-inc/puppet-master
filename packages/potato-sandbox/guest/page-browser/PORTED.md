# PORTED.md: guest/page-browser

Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7. Copyright 2026 Rakazo contributors,
Apache-2.0 (see `packages/potato-sandbox/LICENSE-APACHE-2.0`).

| our file | their file | what changed | lines kept/removed |
|---|---|---|---|
| `pm-sbx-page-browser` | `infra/sandboxes/computer/rakazo-page-browser` | Payment, card, pay-token, payment-frame and wallet paths removed (see below). Renamed rakazo-* to pm-sbx-*, RAKAZO_* to PM_SBX_*, `window.__rakazoPageBrowser` to `window.__pmSbxPageBrowser`, isolated world `pm-sbx-page-browser`, default home `/home/pm`. Default display `:99` (CDP port 9320). CDP connect refuses any host but `127.0.0.1`. Docstring no longer names a host provider. Written with LF line endings. | 2276 of 2670 source lines unchanged; 394 removed or rewritten; output 2343 lines |
| `test_page_browser.py` | `infra/sandboxes/computer/test_page_browser.py` | 22 payment tests removed, plus the payment helper classes (CardBrowser, PayTokenBrowser, urlsplit_origin, card-frame helper). 1 test rewritten as a hidden-frame test without payment frames. 2 tests added (loopback-only connect; CDP port follows DISPLAY, default 99). `Confirm payment` and `Pay` labels in unrelated tests renamed. Loads `pm-sbx-page-browser`. One test skipped on Windows (`os.mkfifo`). | 1781 of 2148 source lines unchanged; 367 removed or rewritten; output 1825 lines |

## What was removed from the helper

- Constants: `PAYMENT_FRAME_ORIGINS`, `CARD_AUTOCOMPLETE`, `NOT_ON_MERCHANT`, `CARD_FIELD_MISSING`, `CARD_FIELD_AMBIGUOUS`, `PAYMENT_FRAME_HINT`, `PAY_TOKEN_FRAME_ORIGIN`, `PAY_TOKEN_MISSING`.
- Functions and classes: `payment_frame_origin`, `pay_token_hint`, `CardFieldUncertain`, `payment_frames`, `pay_token_frames`, `find_card_field`, `merchant_check`, `fill_pay_token`, `fill_card`, `clear_or_uncertain`, `clear_card`.
- In-page script: card masking (`maskCard`, `unmaskCard`), card field lookup (`cardFields`, `findCard`, `hasCardField`, `focusCard`, `cardHolds`, `cardTyped`, `emptyCard`), and the pay-token helpers (`payTokenAccount`, `setPayToken`).
- `fill()` in the page script: the `card` field branch and its checks.
- `snapshot()`: the payment-frame listing, card hint, pay-token hint and the extra page call that read the pay-token account.
- `frame_tree` and `page_frames`: the exemption that skipped payment-provider frames.
- `act()`: the `pay_token` kind, the ref-less card fill, and the card-origin check.

## What was kept

navigate, snapshot (page text plus up to 80 interactive elements with refs), act kinds click / fill / type / upload,
password value masking and saved-login masking, stale-ref rejection, frames (same-process and out-of-process, with
frame refs), uploads (confined to the home folder or team roots), and the desktop fallback hints
(`"fallback": "computer_act"`, "Input type requires desktop tools", "Embedded frames require desktop tools.").

Saved-login fill (`fill` with an `origin`, `field` username or password) is kept. It is not payment, and the brief
did not list it for removal. It can be cut in one step if the lead wants no credential path here.

## Checks

- Payment symbols, before: 395 matches (helper 224, test 171) using
  `\b(card*|pay*|CARD_*|PAYMENT*)|merchant|checkout|stripe|adyen|braintree|shopify|wallet|purchase|cvc|postal-code|cc-(number|exp|csc)`,
  case-insensitive, excluding the word `payload`.
- Payment symbols, after: 0 in code. The only matches are 9 in the helper's and 6 in the test's provenance header,
  which describe what was removed.
- `python -I -m py_compile` on both files: OK.
- `python -I -m unittest` (discovery, from this folder): Ran 124 tests, OK, skipped=1.
- `node --check` on the extracted in-page script (`EVAL_HELPERS`, 684 lines): OK.
- No `rakazo`/`RAKAZO` outside the provenance headers. No personal paths or user names in either file.

## Open items for the lead

- Display default `:99` makes the CDP port default 9320 (`9221 + display`). The browser launch script must use the
  same formula, or set `PM_SBX_CDP_PORT`.
- Hint strings still name `browser_navigate` and `computer_act`. These must match the tool names our supervisor exposes.
- The brief lists `select`, `press` and `scroll` as act kinds. This helper has no such kinds: a select is filled through
  `fill` (`chooseOption`), keys go through `type`, and scrolling is not in this helper. Nothing was invented.
- Not run: anything that needs a real browser. All 124 tests use stand-in CDP clients, so none is browser-dependent.
  The one skip is `test_upload_rejects_fifo_without_opening_it`, which needs `os.mkfifo` and runs in the Linux guest.

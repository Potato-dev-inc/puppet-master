# How every plan in this folder is written

These plans are written for the weakest AI that might build them. If a step can be misread, it will be. So:

## Rules for the writer

1. **No judgement left to the builder.** Every decision is already made in the plan. Where the founder's decision is
   still open, write the default and mark it `DECISION-OPEN (default: ...)`. Never write "choose", "consider", "maybe",
   "as appropriate", "etc." or "and so on".
2. **Exact paths.** Every file is named by its full path from the repo root. Say `create` or `edit`. For an edit, name
   the function or the line to change and show the exact before/after.
3. **Real code.** Every new type, function and command has its full signature, written out in the plan's language
   (Rust, TypeScript, TOML, SQL). Give the whole file when it is under 80 lines; otherwise the full signatures plus
   the body of anything that is not obvious. Names must match the surrounding code (read it first, copy its style).
4. **Verified against the repo.** The writer READS the existing code before writing a step and says which files it
   read. Any claim like "the function X exists" is checked with a search, never remembered.
5. **Small slices.** Each plan is split into SLICES. One slice = one session of one cheap model. A slice touches at
   most 5 files, has exactly one goal, and ends with a command whose output proves it works.
6. **Every slice has these headings, in this order:**
   - `Goal` (one sentence)
   - `Read first` (files to open before touching anything, with why)
   - `Files` (create / edit list)
   - `Steps` (numbered, one action each)
   - `Code` (the exact code or signatures)
   - `Tests to write` (name of each test, the input, the exact expected output)
   - `Run` (the exact commands, with the expected result such as `test result: ok. 12 passed`)
   - `Done when` (a checklist of observable facts)
   - `Do NOT` (the traps: files not to touch, shortcuts not to take, rules from the repo guards)
   - `If it fails` (the 3 most likely failures and the fix for each)
7. **Order and dependencies.** Each slice says which slices must be merged first. A slice never needs something a
   later slice creates.
8. **Plain words for people.** Every string the user sees is written out in full, in simple English (see
   `PRODUCT.md` and `docs/design/` for voice). No jargon in UI text.
9. **Repo rules that apply to every slice** (copy them into `Do NOT` where relevant):
   - Never run a dev server or app on the primary screen; use the preview tools or a headless run.
   - Use a separate `CARGO_TARGET_DIR` (for example `D:/pm-night/target-<slice>`), never the default one.
   - Never kill a process by image name; stop only a PID you started.
   - New Labs flags must also be added to `DEV_LABS_KEYS`.
   - `p2p/guard_tests.rs` and `desk/guard_tests.rs` must stay green: only `iroh_net.rs` touches the network in `p2p/`,
     p2p code prints nothing and reads no env, pairing has no route from agents/MCP/phone.
   - Team traffic must never use the bridge or device pairing.
   - UI is monochrome (see `docs/design/`), no coloured glass.
   - Commit finished work; every handoff has a fresh build id.
10. **Security section.** Every plan ends with threats, what an attacker or a broken peer can do, and the test that
    proves each defence.
11. **Honest status.** The plan ends with `What cannot be verified without real hardware or a second computer`.

## Slice sizing guide
- Haiku-sized: add one command, one component, one pure function with tests, one config field.
- Sonnet-sized: touch a state machine, a protocol, crypto, or more than 3 modules.
- If unsure, split it.

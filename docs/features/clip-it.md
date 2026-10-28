# Clip It: a short video of one finished turn

Code: `packages/app/src-tauri/src/clip/` (the replay, the privacy wall, the plan, the save), `commands/clip.rs`
(three Tauri commands), and in the UI `src/lib/clip*.ts`, `src/lib/mp4Mux.ts` and `src/components/chat/ClipSheet.tsx`.
The layout of every file is in `docs/REPO_MAP.md` section 3.10.

Status: **experimental**. Unit tests and fakes pass in the suites listed below. A live check on a real app run was done on
2026-10-28 (see "Live check" below).

## What and why

A turn just finished and it went well. You want to show it. Clip It makes a short video of that turn. It shows your
request, the agent working on the screen, and the result. It stays on your PC until you share it.

Clip It works for every agent. It replays the pane's own terminal record and the chat items. It does not read an agent's
own file format.

## How to use it

1. In the Chat tab, on the end line of a finished turn, press **Make clip**. The newest finished turn always shows the
   button. Older turns show it when you hover. The same action is in the chat menu as **Make clip of last turn**.
2. The sheet **Make a clip** shows the video being drawn: a progress bar, then **Drawing 40%** and so on. **Cancel** stops it.
3. Your request is the caption at the start. You can edit it (up to 140 characters). Pressing Enter re-draws the video.
4. Watch the preview. Under it, the sheet says how many lines were hidden, for example "Hid 3 lines that looked private."
5. **Save video…** picks a file with the system save dialog. Then the sheet says "Saved to <path>. Nothing was sent anywhere."
   Save again to make another copy. **Close** leaves without saving.

Look at the video before you share it. It shows what the agent printed on the screen.

## What the video looks like

- **Length:** up to 28.5 seconds (the sheet shows the real length, for example "22.5 s"). It is silent (there is no music in this version).
- **Size:** 1280 x 720, 30 frames a second. An H.264 MP4 when your system can make one. Otherwise a WebM or MP4 made in
  real time, which takes about 25 seconds.
- **Title (3 s):** the agent's name and your request.
- **Replay (4 to 18 s):** the terminal as the agent drew it, sped up. The text is drawn as large as the frame allows
  (it fills the width, never under 12 px; a very tall terminal shows its bottom rows). The replay goes on a little past
  the end of the turn, while the agent is still drawing its answer. Pauses are cut to 0.4 seconds at most. A badge says
  "Sped up Nx". Up to 4 short captions say what the agent did, in plain words, such as "Read 6 files · Ran 2 commands".
  A failed command adds "· 1 failed".
- **Result (5 s):** "N files changed", the lines added and removed, up to 6 file names (last names only), and the
  checks line if the turn had one. A turn that changed nothing says "No files changed" and the one-line sentence. A turn
  with nothing to say here (no files fact, no checks, no sentence) has no result scene.
- **End card (2.5 s):** "Made with Puppet Master".
- A small "Puppet Master" mark is on every scene.

If the terminal record is not there (the app was restarted, or the pane was closed long ago), the replay becomes a
list of the steps the agent took. The sheet says so. If there is nothing at all, the sheet says "There is nothing to show for this turn."

## What is hidden

Before any frame is drawn, the text goes through these walls, in Rust first and then in the webview:

- **Known secret shapes:** API keys with the `sk-` prefix, GitHub tokens (`ghp_`, `github_pat_`), AWS key ids (`AKIA`),
  Slack tokens (`xox`), JWTs (starting `eyJ`), private key blocks, and long random runs with no spaces.
- **Values this app process knows:** environment variable values of 4 or more characters, and entries in the Windows
  Credential Manager (and the toolbelt).
- **E-mail addresses.**
- **Folder paths:** an absolute path is cut to its last name. `C:\Users\you\proj\a.rs` shows as `a.rs`.
- **Split secrets:** a secret that is wrapped over two lines, broken across two rows, or has a colour change in the middle is still hidden.
- **Your keystrokes** are not replayed. Puppet Master's own task text is not replayed either. The agent still draws
  your message on its own screen, and that screen text is replayed and cleaned like any other.
- **A changed row loses its colours.** That is on purpose. Safe is more important than pretty.

A row that was changed counts as one "hidden" line. The number is shown in the sheet.

The webview has a second check with the same shapes. If that check ever fires, the item draws as `[hidden]` and the sheet
says how many items were hidden at the last moment. That check should never fire, because Rust already removed the text.

## What cannot be hidden

- **A secret with no known shape.** A short password the agent printed, or a token in a format Puppet Master does not know,
  is not caught unless the app process already knows the value. The preview is the last check. You can drop the clip.
- **Relative paths and file names.** Only absolute paths are cut. `src/app/config.ts` shows as it was printed.
- **Anything else on the screen.** Code, numbers, comments, names that appear in the text, and the agent's own words as
  they were drawn on the terminal. The replay shows the screen, so it shows what the screen showed.
- **Your request.** It is cleaned, but it is still the caption. Edit it if it holds something private.
- **A permission or sign-in screen** that was on the terminal. It shows in the replay. Nothing is answered on your behalf.

Metadata is removed from the file. The MP4 has no title, author, tool or encoder name, and its times are zero. The
default file name has only the date and time, for example `clip-2026-10-28-1530.mp4`.

## Limits

- The replay uses the pane's recent terminal output, which is kept in memory (about 2 MB per pane). A turn older than that
  falls back to the step list.
- A terminal wider than 133 columns still works, but the text is small. The sheet says so.
- A turn that is still running cannot be clipped: "This turn is still running. Make the clip when it ends."
- A turn with no times (for example, a screen-only record) can only be clipped if it is the newest turn.
- A turn of an hour or more is sped up hundreds of times. The badge shows the speed.
- Per clip: at most 180 frames, a 3 MB plan, 40 steps. The file you save is at most 48 MB.
- The plan is kept for 30 minutes, and at most 4 plans are kept.
- Desktop only. The phone has no Make clip button. There is no upload, no share button and no phone route.
- The video is drawn inside the app's window. Nothing is sent to a server.

## How to test

Automated (from `packages/app`, Rust from `packages/app/src-tauri` with your usual `CARGO_TARGET_DIR`):

- `cargo test --lib clip`: the plan model, the replay, the redaction of planted keys (split over rows, wrapped, colour-split,
  known values), the turn window and its refusal sentences, the store (tokens, file magic, one-time save), and the guard tests
  (no bridge or tool mention, no pane writes).
- `npx vitest run src/lib/clip src/lib/mp4Mux src/components/chat/ClipSheet src/components/chat/clipSheet src/components/chat/ChatExportMenu.clip src/components/chat/SessionChatPanel.clip src/components/chat/SessionChatView.clip`:
  the timeline (never over 30 seconds), the drawing, the encoder with a fake encoder, the MP4 writer (no user-data atoms),
  the second wall, and the sheet's steps. A test also greps the clip files for network calls.

By hand (not run yet):

1. In a test folder, start a short agent turn. Ask the agent to print a made-up string with a `sk-` prefix, for example
   `sk-test-` followed by letters. Use a fake value, never a real key.
2. When the turn ends, press **Make clip**. Watch the preview. The fake string must not show. The sheet should say a line was hidden.
3. Save the file to a folder in the test folder. Open it in a video player. It should be silent, at most 28.5 seconds long,
   and show no fake string.
4. While a second turn is still running, check that the button is not there, and that a clip request is refused.

## Live check (2026-10-28)

One real run in a separate app profile, with two real turns (Claude Code and OpenCode) and a planted fake key, a fake
e-mail and a home path in the working folder.

What passed: the button only on a finished turn and not on a shell pane; the sheet with progress, preview, length and
result; the saved MP4 (H.264, 1280 x 720, 30 fps, moov first, no user-data atoms, no tool or encoder tags); a save with a
wrong extension is refused and nothing is written; no network request except the app's own (blob and data only); after an
app restart the old turn makes the steps clip; the slower way when the video encoder is missing; the second agent type.
OCR of the frames of all 4 saved clips (and of the film made from them) for the planted key, e-mail and user name: **0 hits**.
The saved bytes had none of those words either. The frames show `[REDACTED]`, `<email>` and `config.json` where the
turn printed the planted values.

Found and fixed after that run: an OpenCode replay that was garbled after a resize (the app now records a resize before
the PTY applies it, so the repaint that follows is replayed on the right grid), replay text that was too small, a replay
that stopped before the agent's answer, a "0 files changed" scene, a length label rounded up, and a blank scene in the
steps clip. These are covered by tests.

Not checked live: a terminal 150 columns wide (only the 133-column note is tested), and the save path guard for `..`
(the Windows dialog cleans the path first; the Rust guard is covered by tests).

## Not done yet

- **A live re-check of the fixes above** (the film and the OCR scan were made before them).
- **No music and no audio.** A music bed needs an audio track, which is not built.
- **No square or vertical formats, no GIF, no clip of several turns.** These are later steps.
- **No phone clip.** A phone can not make or send a clip.

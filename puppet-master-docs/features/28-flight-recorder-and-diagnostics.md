# Flight recorder and diagnostic file

**Status:** Works today. Checked live. One limit: a run that you cancel writes no recording.

## What it is
The flight recorder is like the black box of an airplane. Each agent pane keeps a short recording of the last part of what the terminal showed and what was typed into it. When a run fails, the recording is saved to a file. The **diagnostic file** is one .zip that you can save and send to someone who helps you. It holds your settings (with secrets removed), recent app messages and the recordings of failed runs. Nothing is sent anywhere by Puppet Master.

## Why it matters
When an agent fails, you often cannot say what happened. You may have closed the pane, or the screen scrolled away. A recording shows the last minutes. It also shows who typed each input: you, a phone, another agent, or Puppet Master itself. This helps you find problems and helps others to help you, without you copying text by hand.

## How to use it
Recording is automatic. You do not turn it on. To save a diagnostic file:
1. Open **Settings**, then **General**.
2. Find **Save a diagnostic file** and click **Review…**.
3. A list shows every file that would go into the zip, with its size. Examples: a short note, the app version, your settings, the app log, and recordings of failed runs. The list updates by itself while it is open.
4. Click **Show** next to a file to read it before you save. Click **Hide** to close it.
5. Remove the tick from any file you do not want to include.
6. Click **Save…**. Choose where to put the .zip file. The name looks like `puppet-master-diagnostic-20261016-1430.zip`.
7. The message says "Saved to … Nothing was sent anywhere." Send the file yourself if you want to.

To watch a recording, open the `.cast` file with any asciinema player. (asciicast is a plain text format for terminal recordings.)

## How it works (simple)
- Each pane keeps the last 2 MB of output and typed input, with times. Old data is dropped when the buffer is full.
- Each input has a tag that says who wrote it: you at the desktop, a paired phone, an agent over MCP, a task from a run, an answer to a card, a dismissed startup screen, or a nudge from Puppet Master.
- When a run ends as **failed**, the recording is written to `<project>/.puppet-master/operations/<operation>/flight.cast`. Puppet Master keeps the newest 30 recordings per project and deletes older ones. The folder is kept out of Git.
- The zip contains: a README, versions, your settings, the last 400 lines of the app log, and up to the 5 newest recordings.
- **Redaction** (removing secrets) happens before anything leaves the live terminal. The same rule is used for saved transcripts, turn summaries, phone push messages, recordings and the diagnostic file. In the diagnostic file it is strict. It removes API keys and tokens, long strings that look like secrets, and your Windows user name in folder paths. The code is built so that a file writer cannot take raw text. It must take text that was already cleaned.
- Redacted text is marked at the type level. A new place that saves text must use the cleaned text, or the code does not compile.
- The terminal on your screen is never changed. Only what is saved or sent on is cleaned.

## Limits and what is not done yet
- The recording is not a video. It is text with times, so it is small.
- **A cancelled run writes no recording.** Only failed runs do. A run that timed out counts as failed.
- A recording shows only what that pane's terminal showed in its last 2 MB. Older activity is cut, and the file says so.
- Redaction is rule based. It catches known secret shapes and long random strings. A secret in an unusual shape could still pass. Read the files with **Show** before you send them.
- Recordings of a pane exist only for panes started in this run of the app. After a restart, there is nothing to export for an older pane.
- Nothing is uploaded or sent. There is no "send for help" button. You send the file yourself.

## For developers
- Recorder: `packages/app/src-tauri/src/flight/` (`ring.rs` 2 MB ring, `cast.rs` asciicast v2, `freeze.rs` failure hook and `KEEP_CASTS = 30`, `bundle.rs`, `zip.rs`, `applog.rs`).
- Redaction: `packages/app/src-tauri/src/redact/` (`Redacted<T>` type, `text` and `strict` modes, `known.rs` for secrets the process knows, `fanout.rs` for phone events).
- Failure hook: `flight::freeze::on_operation_saved`, only for status `Failed` with an error and a pane.
- UI: `components/settings/blocks/DiagnosticFile.tsx`. Tauri commands used: `diagnosticPreview`, `diagnosticShow`, `diagnosticSave`.
- Input tags: `InputTag` in `flight/mod.rs`, from `WriteOrigin`.
- Recording path: `<project>/.puppet-master/operations/<op>/flight.cast`.

## Related
- [Loop guard and panic stop](27-loop-guard-and-panic-stop.md)
- [Security, bridge and devices](29-security-bridge-and-devices.md)
- [Budgets](24-budgets.md)

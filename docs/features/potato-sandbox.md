# Potato Bot sandbox (Preview)

The sandbox is a small private Linux computer that runs inside Windows. A Potato Bot can use it with a screen, a mouse and a
keyboard (open a browser, type in a terminal, work with files) without touching your own files, programs or accounts. You make it
once in a short wizard. It starts by itself when a bot first needs it and stops by itself when it has been idle.

This page covers what you can do today: set it up, start, stop, reset and remove it, and change when it stops. A bot using it, and
watching or taking over its screen, come in later pieces.

It is off by default. Turn it on in **Settings > General > Labs > Sandbox** (the setting is `potato_sandbox_enabled`). It also needs
**Potato Bot** on. While either is off there is no Sandbox part in Settings and nothing runs.

## Set it up

Open **Settings > Potato Bot > Sandbox** and press **Set up…**. Nothing is installed until you press **Install** on the last step.

1. **Look.** Ubuntu (the standard desktop), Ubuntu with the Zorin look (Zorin's light theme, icons, wallpaper and font, about 400 MB
   more), or the light desktop (smaller, uses the least memory). Always included, whatever you pick: Chrome, a terminal, a file
   manager, a text editor, a PDF viewer, zip, fonts for every language and emoji, copy and paste, and screenshots.
2. **Use.** Coding, Daily use, or both.
3. **Coding** (if picked): the languages are checked for you from your own projects (see below); a package manager (npm, pnpm, Yarn
   or Bun; the one most of your projects use is picked); and an extras checklist (tmux, SQLite, a PostgreSQL client, HTTPie,
   ShellCheck, FFmpeg). Git, Node.js (current LTS), the GitHub tool and a C compiler are always included.
   **Daily use** (if picked): LibreOffice (Writer, Calc, Impress) is always included; you pick a media player and an image viewer.
   Sign-ins for your apps (Gmail, Calendar, Drive and so on) happen inside the sandbox's Chrome. Your sign-ins on this computer are
   not copied into it.
4. **Summary.** Your choices, and an estimate of the download, the disk and the time, always shown as "about". A box saves a reset
   point at the end (on by default). **Install** starts the set up.

The choices on steps 3 are read from the recipe files in `packages/potato-sandbox/recipes/`, so a new recipe option shows up in
the wizard without a change to it.

While it installs you see a bar, the step name (for example "Downloading Ubuntu", then the line the installer prints, such as
"Installing XFCE desktop") and the step count. **Hide** closes the window and set up keeps going; the Sandbox row in Settings shows
the same bar. **Stop set up** stops at the next step and takes away the half-made sandbox, so a new try starts clean. If a step
fails, the screen says why in one sentence and **Try again** goes back to the summary.

### Languages from your projects

The wizard looks at the project folders Puppet Master already knows (your recent projects) for marker files, up to three folders
deep, skipping `node_modules`, `target`, `.git` and similar. It only reads names; it opens no file.

| Language | Marker files |
|---|---|
| Python | `pyproject.toml`, `requirements.txt`, `setup.py`, `Pipfile` |
| Go | `go.mod` |
| Rust | `Cargo.toml` |
| Java | `pom.xml`, `build.gradle`, `build.gradle.kts` |
| .NET | any `*.csproj` |

JavaScript needs no pick because Node.js is always installed; `pnpm-lock.yaml`, `yarn.lock`, `bun.lockb` / `bun.lock` and
`package-lock.json` only decide which package manager is suggested. You can change every pre-checked box.

### If Windows' Linux feature is not turned on

The sandbox needs Windows' Linux feature (WSL). If it is missing, the wizard opens on a step that says so, and the Sandbox row says
"Needs Windows' Linux feature (admin + restart)". The button **Turn on Windows' Linux feature…** opens a confirm dialog that says
Windows will ask for an administrator's OK and that you will most likely have to restart. Only the dialog's **Turn on** button
runs it. Nothing, not an install, a start or a bot, turns it on by itself. After it, restart the computer and open Set up again.

## The Sandbox row

| State | Buttons |
|---|---|
| Not set up | **Set up…** |
| Setting up | the bar, **Show**, **Stop set up** |
| Stopped | **Start**, **Reset…**, **Remove…** |
| Running | **Stop**, **Reset…**, **Remove…** |
| Could not start (the reason is shown) | **Start again**, **Reset…**, **Remove…** |

It also shows how much disk the sandbox uses on this computer and the size of the reset point.

- **Reset…** asks first. It puts the sandbox back to how it was at the end of set up. Everything done in it since is lost, including
  sign-ins. It is off when no reset point was saved.
- **Remove…** deletes the sandbox and its reset point. You must type the word `remove` first; the button stays off until you do, and the
  app checks the word again.
- **Stop when idle** (1 to 240 minutes, default 15): the sandbox stops by itself after this long without use and starts again when a
  bot needs it. The number is kept between runs of the app.

## Safety walls

These come from the sandbox core and its guest files; the wizard only sets them up.

- **One door.** The app starts `wsl.exe` in one place only (`potato/sandbox/wsl.rs`). It refuses any distro that is not named
  `pm-sbx-…`, so your own Ubuntu or Docker's distro can never be touched. It never runs `wsl --shutdown` and never reads or writes
  your WSL settings file.
- **Your files stay out.** Windows drives are not mounted, Windows programs cannot be run from inside, and the Windows graphics
  bridge (WSLg) is cut at boot. The sandbox user has no administrator rights.
- **Network walls.** The sandbox can reach the internet, but not your Windows PC, your home network or other devices on it. The walls
  are proved at every start; if the proof fails the sandbox does not start.
- **Secrets stay in memory.** The control token and the screen passwords are made by the app at each start, pushed in over stdin and
  kept in memory. They are never written to a Windows disk, a log or the page.
- **No window on your screen.** The sandbox draws on its own hidden display.
- **Everything is on your disk, under the app's folder.** The sandbox lives in `<app data>/potato/sandbox/`, never in a project.
- **The page is desktop only.** The commands behind the Sandbox settings are not reachable through the bridge, the phone or MCP.

## Limits

- **Windows only.** The sandbox needs WSL 2 (Windows 10/11). On another system the row says it needs Windows' Linux feature.
- **Size and time.** Set up downloads roughly 1 to 2 GB and takes about 10 to 25 minutes, depending on what you pick and your
  internet. The disk file (about 4 to 7 GB) grows as the sandbox is used; the reset point is a separate copy of nearly the same size
  (the spike measured 3.0 GB for a 3.7 GB disk). The numbers in the wizard come from the spike (`RESULTS.md`); the ones for tools the
  spike did not measure are rough and are shown as "about".
- **Memory.** An idle desktop uses about 0.85 GB of RAM; with Chrome and a few apps about 1.2 GB (spike numbers). Settings does not show
  live memory.
- **Shared Linux VM.** All WSL distros share one VM. Docker Desktop's memory limits (the global WSL settings, which Puppet Master never
  edits) also bound the sandbox, and anything that shuts WSL down stops it.
- **The Zorin look is the look only.** It is XFCE with Zorin's themes, icons, wallpaper and font from the official Zorin package
  source. Zorin OS itself (its own desktop) was fragile on WSL in the spike and is not offered.
- **No sign-in copying yet.** Copying an agent's login into the sandbox is a later choice; today you sign in inside the sandbox's Chrome.
- **The app guest files must ship with the app.** The install reads `packages/potato-sandbox/` from the app's resources or, in a
  dev build, from the source tree.

## How it is wired and shipped

- **One sandbox, one lease.** `potato/sandbox_host.rs` builds the app's one sandbox (the real `WslSandbox`, a stand-in without `wsl.exe`, or `FakeSandbox` in a `dev-fake-tui` build with the stand-in brain) and is the only place that hands it on (`sync`, run after it is built and after every install, start, stop, reset and remove). The Computer panel and the screen links get it as the hub's source; the bot's tools get a `Port` made of the same sandbox and the screen hub's own lease table, so the panel, the tools and the host see one lease. Labs off, or nothing set up yet: both get nothing, and a running sandbox is stopped.
- **No window on your screen.** After every start `potato/window_guard.rs` asks PowerShell (hidden, no file on disk) whether a visible window owned by `msrdc` (WSLg's remote-desktop client) exists. If one does, or if it cannot tell, the start fails, the sandbox is stopped and the Settings row shows an error card.
- **Packaged like the brain.** `scripts/bundle-potato-sandbox.mjs` copies `packages/potato-sandbox/{guest,recipes,viewer}` (the viewer includes noVNC) to `packages/app/src-tauri/resources/potato-sandbox/` (generated, gitignored); `scripts/ensure-tauri-resources.mjs` runs it when a piece is missing and `scripts/bundle-mcp.mjs` runs it for a release. `tauri.conf.json` lists `resources/potato-sandbox/**/*`. `potato/resource_dirs.rs` finds the installed folder (`resources/potato-sandbox`, then `potato-sandbox`); a dev build falls back to the staged copy and then the source tree (`sandbox_host::guest_root`, `screen_live::viewer_dir`).

## Where it is in the code

- Recipes: `packages/potato-sandbox/recipes/` (`zorin-look.json` is new; `coding.json` has the extras checklist; `SCHEMA.md` has
  the `requires` field). The look is applied once at the first XFCE start by `guest/desktop/pm-sbx-zorin-look.sh` (an autostart
  entry). `light.json`'s desktop no longer shows the "Something went wrong while setting the wallpaper" pop-up:
  `pm-sbx-start-desktop.sh` writes a style overlay with no background and a stand-in `fbsetbg`.
- Rust: `packages/app/src-tauri/src/potato/sandbox/manager.rs` (status, the install thread and its events, busy lane, typed remove,
  the confirm token), `detect.rs` (languages from projects), `potato/sandbox_host.rs` (the app's one manager, the idle tick, the
  idle setting file), `commands/potato_sandbox.rs` (the `potato_sandbox_*` commands; the only place a `Consent` is made).
- UI: `components/settings/sandbox/` (`SandboxWizard.tsx`, `SandboxRows.tsx`, `useSandbox.ts`), `lib/potato-sandbox.ts` (types,
  the estimate, the fake), `styles/potato-sandbox.css`. Events: `potato://sandbox-install` (`{ install }`) and
  `potato://sandbox-changed`.
- Tests: `cargo test --lib potato` (manager, detect, install, guards, commands) and `npx vitest run src/components/settings/sandbox
  src/lib/potato-sandbox.test.ts`.

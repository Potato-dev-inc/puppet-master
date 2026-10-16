# Agent wizard (teach Puppet Master a new agent)

**Status:** Preview · In `main`, not in a release yet (after 0.2.1). Phase 1 only: agents that can run one task and print the answer without their full-screen screen.

## What it is
You type the command you use to start a CLI agent, for example `kiro-cli`. Puppet Master (PM) reads the program's help, asks your own Claude Code (or Codex, or an OpenRouter model) to write the settings file (TOML) for it, checks the file, tests it on the real program, and gives it to you to approve. You never have to write TOML yourself.

## Why it matters
Before, adding a new agent meant writing a TOML file by hand (see [Supported agents](12-supported-agents.md)). New CLI agents come out every month. With the wizard, any agent that has a "run one task" mode can be added in a few minutes, and it then works in the **+** menu and for other agents over MCP.

## Before you start
1. **Install the agent** and make sure its command works in a terminal.
2. **Sign in to it** yourself. The wizard never signs in for you.
3. **Check it has a one-task mode.** For example `xyz run "task"`, `xyz -p "task"` or `xyz chat --no-interactive "task"`. A JSON output option (like `--format json`) is even better. An agent that only works full-screen needs Phase 2, which is not built yet.
4. **Have Claude Code working.** It writes the file. (Codex also works, if it is healthy on your PC.)

## How to use it
1. Open **Settings > Agents**. Find **Your own agents**. Press **Add with wizard**.
2. **"Which command starts it?"** Type the command, for example `kiro-cli`. The wizard shows where it found the program and its version.
3. **"Who writes the profile?"** Pick **Claude Code** (or Codex). To use an OpenRouter model, you first see exactly what will be sent, and you must agree.
4. **Wait.** A progress list shows: Read the help, Draft, Check, Fix. This takes about a minute per round, up to 5 rounds.
5. **Live test.** The wizard shows the exact commands it will run and the cost (about 4 tiny messages on the new agent's account). Press **Run the live test**. If a fix changes the commands, it asks you again.
6. **Review.** You see the command and what was tested, for example "Tested live on kiro-cli 2.29.1: 4 of 4 checks passed".
   - **Approve** turns the agent on (the normal approval dialog).
   - **Show TOML** shows the file.
   - **Retry** or **Discard** if you are not happy.
7. The new agent is in the **+** menu, in **Set up agents**, and other agents can start it with `run_agent`.

**If it stops with "&lt;agent&gt; itself returned an error":** the problem is in the agent's own setup (sign-in, model or provider), not in the profile. Fix it in the agent, then press **Retry**. Retry only runs the live test again.

## How it works (simple)
- **Read the help.** PM runs only `--version`, `--help`, `-h`, `help`, `<sub> --help` and `help <sub>`. Each run has 5 seconds, gets no input, and runs in an isolated temporary folder, never your project. Secrets are removed from the text. Risky names like `install`, `login` or `delete` are never run.
- **Draft.** Your Claude Code gets a fixed prompt: the format guide, a worked example, and the help text. It answers with one TOML file. It is told not to run any tools.
- **Check.** PM checks the file with the same rules every agent must pass. Problems go back to the AI word for word, and it fixes them.
- **Live test.** PM runs the real program 4 times with tiny messages: a random word, the same conversation again (resume), a short sentence, and a wrong model name (it must be seen as an error). The AI never sees the random words, so it cannot cheat.
- **Save.** The file goes to `%APPDATA%\com.puppetmaster.app\agents\<id>.toml`, **turned off** until you approve it. It never overwrites a file. The real test outputs are saved next to it as test files. A small `<id>.wizard.json` notes who wrote it and what passed.

## Safety
- Nothing is turned on without your **Approve**.
- Before the live test, you see the exact commands. PM refuses to run anything else.
- The live test only runs the program you typed, in an isolated temporary folder.
- Agents cannot use the wizard over MCP, and the phone cannot use it. Only the desktop app can.

## Limits and what is not done yet
- **Phase 1 only.** Agents with only a full-screen screen (no one-task mode) cannot be added yet.
- Not in a release yet. Run it from `main`, or wait for the next release.
- "Upgrade" for agents added with **Add as generic agent**, and an entry in **Set up agents**, are not built yet.
- **Tested live:** Kiro (passed in round 2). Kilo: the profile was right in round 1, but Kilo itself returned a provider error on the test PC. No other agents were tried yet.

## For developers
- Rust: `src-tauri/src/tui/declarative/wizard/` (probe, verify, live test, finish). Bridge routes `/agents/wizard/*`, local app only.
- Frontend: `src/components/settings/blocks/AgentWizard.tsx`, `src/lib/agentWizard.ts` (draft loop), `src/lib/agentWizardPrompt.ts` (prompt, version 4).
- Plan and spec: `docs/specs/agent-wizard/PLAN.md`, `SPEC.md` (section 10.1 has the benchmark commands).

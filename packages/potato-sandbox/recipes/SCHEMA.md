# Sandbox desktop recipes

Each `*.json` file here is one recipe. `guest/desktop/apply-recipe.py` reads them and installs the chosen
ones inside the PM Ubuntu 24.04 distro, as root. Nothing here runs on Windows.

- `base.json` is always installed.
- `light.json` and `xfce.json` are the two desktops. Only one may be picked (`exclusive: "desktop"`).
- `coding.json` and `daily.json` are optional add-ons.
- `zorin-look.json` is an optional look for the XFCE desktop. It has `"requires": "xfce"`: choosing it without `xfce` is an error.

Try the plan without installing anything (any OS, no root):

    python -I guest/desktop/apply-recipe.py --recipes-dir recipes --dry-run \
        --recipe xfce --recipe coding --opt coding.pkg=pnpm --opt coding.languages=python,go

Install in the distro, as root (`wsl -d <distro> -u root -- ...`, script sent over stdin, never as `bash -c "$VAR"`):

    python3 -I /usr/local/share/pm-sbx/desktop/apply-recipe.py --recipe xfce --recipe coding \
        --opt coding.pkg=pnpm --opt coding.languages=python,go

`-I` keeps the script folder off `sys.path` and ignores `PYTHON*` variables.

Menu as JSON for the app: `apply-recipe.py --list`.

## File shape

```
{
  "_header" or "_source": "text kept for provenance. Keys that start with _ are ignored by the installer.",
  "id": "xfce",                       must equal the file name (letters, digits, dashes)
  "title": "XFCE desktop",           short name, shown to the user
  "description": "...",              one or two plain sentences
  "exclusive": "desktop" | null,     two recipes with the same value cannot both be picked
  "requires": "xfce" | null,         optional: another recipe that must be picked too
  "session": "fluxbox" | "xfce4",    desktop recipes only; written to /etc/pm-sbx/desktop.conf
  "pre":  [ "shell line", ... ],     run as root before the apt step (for example adding an apt repo)
  "apt":  [ "package", ... ],        apt-get install -y --no-install-recommends
  "debs": [ { "url": "https://...", "sha256": null | "64 hex" } ],
  "post": [ "shell line", ... ],     run as root after the packages
  "size_mb": 300,                    optional: about how many MB this step downloads. Only steers the progress bar
                                     (a big step moves it more). Without it: 4 + 3 per package + 20 per .deb + 0.5 per post line.
  "early_pre": true,                 optional: the pre lines need only what a stock Ubuntu image has (curl, gpg, ca-certificates),
                                     so they may run, and this step's packages may download, while an earlier step is still
                                     installing. Leave it out when a pre line needs something an earlier step installs.
  "options": [ ... ]                 see below; [] when there are none
}
```

Rules that the installer enforces:

- URLs must be `https://`. A redirect to another scheme stops the step.
- `sha256` null means "not pinned yet". The installer logs the real hash (`UNPINNED` line in the log) so it can be pinned later.
- Package names must match `[a-z0-9][a-z0-9.+-]*`.
- Shell lines run one at a time, each in a fresh `bash -e -o pipefail -c`. Nothing carries over between lines.
- Shell lines may use `$PM_SBX_ASSETS` (the folder with the PM guest files). Do not put user data or secrets in lines.
- If a recipe has `pre` lines, the installer runs `apt-get update` after them.
- Every step must be safe to run again. The installer skips a unit whose content (and the guest files) has not changed since the last success. A changed step runs again.

## Options

An option is a choice the user makes. It can be one of three kinds.

| kind | value on the command line | default |
|---|---|---|
| `toggle` | `--opt recipe.opt` (on) or `--opt recipe.opt=false` (off) | `"default": true/false` |
| `single` | `--opt recipe.opt=choice-id` | `"default": "choice-id"` (or the first choice) |
| `multi` | `--opt recipe.opt=a,b` (empty value = none) | `"default": ["a", ...]` |

A toggle has its own `apt`, `debs`, `pre`, `post`. A single or multi option has `choices`. Each choice has
`id`, `title`, and its own `apt`, `debs`, `pre`, `post`. A choice with no lines (for example `"none"`) installs nothing.

Option defaults apply whenever the recipe is installed and the option is not named on the command line.
Naming an option of a recipe that is not in the list includes that recipe too.

## Progress output

In a real run the installer prints a steady stream of

    PROGRESS <n>/1000 <text>

`n` out of 1000 is the share of the whole job that is done. It never goes down and is 1000 only on the last line.
The text says what is happening, for example `Desktop base: Downloading 120 of 340 MB · 4.1 MB/s · about 1 min left`,
`XFCE desktop: Unpacking 11 of 42 · libxfce4ui-2-0`, `Coding tools: Setting up 40 of 42 · build-essential`.
A step's share of the bar comes from its `size_mb`. Inside a step the time is split: refresh (3), download (45),
install (30), post lines (0.4 each, at most 6).
A dry run (`--dry-run`) counts steps instead (`PROGRESS 2/5 Would install ...`).

The last line is `RESULT {...}` (JSON with `status`, `recipes`, `options`, `installed`, `skipped`, `mirror`,
`downloaded_mb`, `seconds`). Errors print `ERROR <text>` and exit 1. Full command output is in
`/var/log/pm-sbx/apply-recipe.log`.

Exit codes: 0 ok, 1 a step failed, 2 bad recipe or option, 3 not root, 4 the `pm` user has sudo rights.

## Speed

What the installer does so a fresh sandbox is ready sooner (all of it is in `apply-recipe.py`, and each part has a test in
`guest/desktop/test_apply_recipe.py`):

- **Nearest fast mirror.** Before the first `apt-get update` it reads the GeoIP list from `mirrors.ubuntu.com/mirrors.txt` and
  the list for the country of the PC's time zone (`<CC>.txt`), asks every mirror plus `archive.ubuntu.com` for `noble-updates/InRelease`
  (answer time and date), drops a mirror that is more than a day behind, reads 400 KB from the three quickest, and writes
  the winner into `/etc/apt/sources.list.d/ubuntu.sources` (the security stanza too when the mirror carries `-security`, else
  `security.ubuntu.com`). The pick takes at most about 5 seconds. Offline or no answer: the archive stays. The first file is kept in
  `/var/lib/pm-sbx/ubuntu.sources.orig`.
- **Several downloads at once.** `apt-get install --print-uris` says what is needed; the installer fetches those `.deb` files over 8
  keep-alive connections (biggest first), checks size and SHA-256 and puts them in `/var/cache/apt/archives`; Chrome (a `debs` entry)
  downloads beside them, and starts during the first list refresh. `apt-get install` then finds everything there. A file that failed
  is left to apt. This is also where the speed and the time left come from.
- **Download the next step while this one installs.** When a step starts to unpack, a thread already looks at the next step that installs
  packages: it fetches its `.deb` files (and, with `early_pre`, runs its `pre` lines and the list refresh first), then the step after
  that. The network is not idle while dpkg works. A step with `pre` lines and no `early_pre` stops the chain. A failure there
  is only logged; the step then does its own work.
- **No Recommends, no Suggests, no language files.** `--no-install-recommends` on every install; `/etc/apt/apt.conf.d/90pm-sbx` sets
  `Acquire::Languages "none"`, retries 3, pipelining 10, no Suggests and waits for the dpkg lock (3 minutes). `Acquire::Queue-Mode` is
  left alone: `access` would put every host in one queue.
- **Quiet guest.** `packagekit`, `unattended-upgrades` and the `apt-daily` jobs are masked (and stopped when systemd runs). The app also puts
  `guest/desktop/wsl-install.conf` (systemd off) in place and restarts the distro once before the installer starts, so none of them runs.
- **dpkg without fsync** (`--force-unsafe-io`, for the install only), one apt run per step for packages and `.deb` together (one set of triggers).
- **noVNC without its dependencies.** Ubuntu's `novnc` package depends on `nodejs` and `python3-novnc` (OpenStack oslo libraries, pillow,
  babel, netaddr: about 60 packages, 65 MB). `guest/desktop/pm-sbx-novnc-lite.sh` repackages the stock `.deb` without its `Depends` and
  installs that (version suffix `+pmsbx1`, held), so `/usr/share/novnc` is there and nothing else comes with it.

## Guest files

`apply-recipe.py` expects the PM guest files in `/usr/local/share/pm-sbx/desktop` (a copy of `guest/desktop/`)
and the recipes in `/usr/local/share/pm-sbx/recipes`. `base.json` copies the files into place:
`/etc/wsl.conf`, `/usr/local/sbin/pm-no-wslg.sh`, `/etc/profile.d/pm-sandbox.sh`, `/usr/local/bin/pm-sbx-start-desktop`,
`/usr/local/bin/pm-sbx-browser`, the browser desktop entry and the Chrome policy file. A new `/etc/wsl.conf`
takes effect only after the distro restarts, so the host must run `wsl --terminate <distro>` after the first apply
(the installer does not do it, because it runs inside the distro).

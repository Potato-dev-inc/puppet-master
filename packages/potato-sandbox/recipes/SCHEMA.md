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

For each step the installer prints, in order:

    PROGRESS <done>/<total> Installing <title>
    PROGRESS <done+1>/<total> Installed <title>

Steps that were already in place print `<title>: already in place`. The last line is
`RESULT {...}` (JSON with `status`, `recipes`, `options`, `installed`, `skipped`). Errors print
`ERROR <text>` and exit 1. Full command output is in `/var/log/pm-sbx/apply-recipe.log`.

Exit codes: 0 ok, 1 a step failed, 2 bad recipe or option, 3 not root, 4 the `pm` user has sudo rights.

## Guest files

`apply-recipe.py` expects the PM guest files in `/usr/local/share/pm-sbx/desktop` (a copy of `guest/desktop/`)
and the recipes in `/usr/local/share/pm-sbx/recipes`. `base.json` copies the files into place:
`/etc/wsl.conf`, `/usr/local/sbin/pm-no-wslg.sh`, `/etc/profile.d/pm-sandbox.sh`, `/usr/local/bin/pm-sbx-start-desktop`,
`/usr/local/bin/pm-sbx-browser`, the browser desktop entry and the Chrome policy file. A new `/etc/wsl.conf`
takes effect only after the distro restarts, so the host must run `wsl --terminate <distro>` after the first apply
(the installer does not do it, because it runs inside the distro).

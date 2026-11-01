#!/usr/bin/env python3
# apply-recipe.py - installs the PM sandbox desktop recipes inside the distro (run as root).
# Puppet Master original (not a Rakazo port). Stdlib only. Idempotent: a step whose content
# (and the guest files it copies) did not change since the last successful run is skipped.
#
# Install, inside the distro, as root:
#   python3 -I /usr/local/share/pm-sbx/desktop/apply-recipe.py --recipe xfce --recipe coding \
#       --opt coding.pkg=pnpm --opt coding.languages=python,go
# Plan only (any OS, no root, changes nothing):
#   python -I apply-recipe.py --recipes-dir ../recipes --dry-run --recipe xfce --recipe coding \
#       --opt coding.pkg=pnpm --opt coding.languages=python,go
# Read the menu:
#   python3 -I apply-recipe.py --list
#
# stdout (the app reads this): "PROGRESS <n>/<total> <text>" lines, "ERROR <text>" on failure,
# and one final "RESULT <json>" line. Full command output goes to the log file.
# Exit codes: 0 ok, 1 a step failed, 2 bad recipe or option, 3 not root, 4 the desktop user has sudo.

import argparse
import glob
import hashlib
import json
import os
import re
import shlex
import subprocess
import sys
import urllib.parse
import urllib.request
from pathlib import Path

DEFAULT_RECIPES_DIR = "/usr/local/share/pm-sbx/recipes"
DEFAULT_ASSETS_DIR = "/usr/local/share/pm-sbx/desktop"
# Folders next to the assets folder that post lines read through $PM_SBX_GUEST (the copy of the package's guest/ folder).
GUEST_SUBDIRS = ("egress", "control", "page-browser")
DEFAULT_STATE_FILE = "/var/lib/pm-sbx/recipes-state.json"
DEFAULT_LOG_FILE = "/var/log/pm-sbx/apply-recipe.log"
DESKTOP_CONF = "/etc/pm-sbx/desktop.conf"
DEB_CACHE = "/var/cache/pm-sbx/debs"
ALWAYS = "base"
DESKTOP_USER = "pm"
SESSIONS = ("fluxbox", "xfce4")
MAX_DEB_BYTES = 600 * 1024 * 1024
BLOCK_KEYS = ("pre", "apt", "debs", "post")

ID_RE = re.compile(r"^[a-z][a-z0-9-]{0,47}$")
PKG_RE = re.compile(r"^[a-z0-9][a-z0-9.+-]{0,127}$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
DEB_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+~-]*\.deb$")
TRUE_WORDS = ("true", "1", "yes", "on")
FALSE_WORDS = ("false", "0", "no", "off")
KINDS = ("toggle", "single", "multi")

EXIT_OK, EXIT_STEP, EXIT_USAGE, EXIT_ROOT, EXIT_SUDO = 0, 1, 2, 3, 4


class UsageError(Exception):
    """Bad recipe, option or command line. Exit 2."""


class StepError(Exception):
    """A command or download failed. Exit 1."""


class PreflightError(Exception):
    """The desktop user has more rights than it may have. Exit 4."""


def progress(done, total, text):
    text = " ".join(str(text).split())
    print("PROGRESS %d/%d %s" % (done, total, text), flush=True)


def emit_error(text):
    text = " ".join(str(text).split())
    print("ERROR " + text, flush=True)
    print("ERROR " + text, file=sys.stderr, flush=True)


def emit_result(obj):
    print("RESULT " + json.dumps(obj, sort_keys=True), flush=True)


def sha256_file(path):
    digest = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1 << 16), b""):
            digest.update(chunk)
    return digest.hexdigest()


def guest_dir_of(assets_dir):
    """The guest/ folder copy: the parent of the desktop assets folder."""
    return str(Path(assets_dir).resolve().parent)


def assets_digest(path):
    """Hash of the assets folder plus the guest subfolders the post lines copy from, so a changed file re-runs its step."""
    root = Path(path)
    if not root.is_dir():
        return "missing"
    digest = hashlib.sha256()
    roots = [root] + [root.resolve().parent / name for name in GUEST_SUBDIRS]
    for base in roots:
        if not base.is_dir():
            continue
        digest.update(base.name.encode("utf-8") + b"\1")
        for item in sorted(base.rglob("*")):
            if item.is_file() and "__pycache__" not in item.parts:
                digest.update(str(item.relative_to(base)).encode("utf-8"))
                digest.update(b"\0")
                digest.update(item.read_bytes())
                digest.update(b"\0")
    return digest.hexdigest()


def unit_digest(unit, assets_hash):
    blob = json.dumps({"unit": unit, "assets": assets_hash}, sort_keys=True).encode("utf-8")
    return hashlib.sha256(blob).hexdigest()


# ---------------------------------------------------------------- recipe loading and checks

def check_block(block, where):
    if not isinstance(block, dict):
        raise UsageError("%s: must be an object" % where)
    for key in ("pre", "post"):
        lines = block.get(key, [])
        if not isinstance(lines, list) or not all(isinstance(x, str) for x in lines):
            raise UsageError("%s.%s: must be a list of shell lines" % (where, key))
    packages = block.get("apt", [])
    if not isinstance(packages, list) or not all(isinstance(x, str) and PKG_RE.match(x) for x in packages):
        raise UsageError("%s.apt: must be a list of package names" % where)
    debs = block.get("debs", [])
    if not isinstance(debs, list):
        raise UsageError("%s.debs: must be a list" % where)
    for index, deb in enumerate(debs):
        if not isinstance(deb, dict) or not isinstance(deb.get("url"), str) or not deb["url"].startswith("https://"):
            raise UsageError("%s.debs[%d]: needs an https url" % (where, index))
        sha = deb.get("sha256")
        if sha is not None and not (isinstance(sha, str) and SHA_RE.match(sha)):
            raise UsageError("%s.debs[%d].sha256: must be null or 64 lower-case hex" % (where, index))


def block_of(obj):
    return {key: list(obj.get(key, [])) for key in BLOCK_KEYS}


def toggle_default(opt):
    return bool(opt.get("default", False))


def choice_ids(opt):
    return [choice["id"] for choice in opt["choices"]]


def option_default(opt):
    if opt["kind"] == "toggle":
        return toggle_default(opt)
    if opt["kind"] == "single":
        return opt.get("default", choice_ids(opt)[0])
    return list(opt.get("default", []))


def validate_recipe(rid, data):
    if not isinstance(data, dict):
        raise UsageError("%s: recipe file must hold a JSON object" % rid)
    if data.get("id") != rid:
        raise UsageError("%s: the id inside the file must be '%s'" % (rid, rid))
    for key in ("title", "description"):
        if not isinstance(data.get(key), str) or not data[key].strip():
            raise UsageError("%s.%s: required text" % (rid, key))
    exclusive = data.get("exclusive")
    if exclusive is not None and not isinstance(exclusive, str):
        raise UsageError("%s.exclusive: must be a string or null" % rid)
    session = data.get("session")
    if session is not None:
        if session not in SESSIONS:
            raise UsageError("%s.session: must be one of %s" % (rid, ", ".join(SESSIONS)))
        if exclusive != "desktop":
            raise UsageError("%s: a session recipe must have exclusive 'desktop'" % rid)
    check_block(data, rid)
    options = data.get("options", [])
    if not isinstance(options, list):
        raise UsageError("%s.options: must be a list" % rid)
    seen = set()
    for opt in options:
        if not isinstance(opt, dict):
            raise UsageError("%s.options: each option must be an object" % rid)
        oid = opt.get("id")
        if not isinstance(oid, str) or not ID_RE.match(oid) or oid in seen:
            raise UsageError("%s: bad or repeated option id %r" % (rid, oid))
        seen.add(oid)
        where = "%s.%s" % (rid, oid)
        for key in ("title", "description"):
            if not isinstance(opt.get(key), str) or not opt[key].strip():
                raise UsageError("%s.%s: required text" % (where, key))
        if opt.get("kind") not in KINDS:
            raise UsageError("%s.kind: must be one of %s" % (where, ", ".join(KINDS)))
        if opt["kind"] == "toggle":
            if not isinstance(opt.get("default", False), bool):
                raise UsageError("%s.default: must be true or false" % where)
            check_block(opt, where)
            continue
        choices = opt.get("choices")
        if not isinstance(choices, list) or not choices:
            raise UsageError("%s.choices: needs at least one choice" % where)
        cseen = set()
        for choice in choices:
            if not isinstance(choice, dict):
                raise UsageError("%s.choices: each choice must be an object" % where)
            cid = choice.get("id")
            if not isinstance(cid, str) or not ID_RE.match(cid) or cid in cseen:
                raise UsageError("%s: bad or repeated choice id %r" % (where, cid))
            cseen.add(cid)
            if not isinstance(choice.get("title"), str) or not choice["title"].strip():
                raise UsageError("%s.%s.title: required text" % (where, cid))
            check_block(choice, "%s.%s" % (where, cid))
        default = option_default(opt)
        if opt["kind"] == "single" and default not in cseen:
            raise UsageError("%s.default: '%s' is not a choice" % (where, default))
        if opt["kind"] == "multi":
            if not isinstance(default, list) or not all(d in cseen for d in default):
                raise UsageError("%s.default: must be a list of choice ids" % where)


def load_recipe(recipes_dir, rid):
    if not ID_RE.match(rid):
        raise UsageError("bad recipe id '%s'" % rid)
    path = Path(recipes_dir) / (rid + ".json")
    if not path.is_file():
        raise UsageError("unknown recipe '%s' (no %s)" % (rid, path))
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as err:
        raise UsageError("cannot read recipe '%s': %s" % (rid, err))
    validate_recipe(rid, data)
    return data


def parse_opts(items):
    """'recipe.option=value' (or 'recipe.option' for a toggle) -> {recipe: {option: raw}}."""
    out = {}
    for item in items:
        key, equals, raw = item.partition("=")
        rid, dot, oid = key.partition(".")
        if not dot or not ID_RE.match(rid) or not ID_RE.match(oid):
            raise UsageError("bad --opt '%s': use recipe.option=value" % item)
        out.setdefault(rid, {})[oid] = raw if equals else "true"
    return out


def make_unit(key, title, block):
    unit = {"key": key, "title": title}
    unit.update(block_of(block))
    if not any(unit[k] for k in BLOCK_KEYS):
        return None
    return unit


def resolve_option(opt, raw):
    where = opt["id"]
    if opt["kind"] == "toggle":
        word = raw.strip().lower()
        if word in TRUE_WORDS:
            return True
        if word in FALSE_WORDS:
            return False
        raise UsageError("option %s needs true or false, got '%s'" % (where, raw))
    if opt["kind"] == "single":
        if raw not in choice_ids(opt):
            raise UsageError("option %s: '%s' is not one of %s" % (where, raw, ", ".join(choice_ids(opt))))
        return raw
    wanted = [part.strip() for part in raw.split(",") if part.strip()]
    for part in wanted:
        if part not in choice_ids(opt):
            raise UsageError("option %s: '%s' is not one of %s" % (where, part, ", ".join(choice_ids(opt))))
    return [cid for cid in choice_ids(opt) if cid in wanted]


def build_units(rid, recipe, raw_opts):
    known = {opt["id"] for opt in recipe.get("options", [])}
    unknown = sorted(set(raw_opts) - known)
    if unknown:
        raise UsageError("%s has no option %s" % (rid, ", ".join(unknown)))
    units = []
    chosen = {}
    head = make_unit(rid, recipe["title"], recipe)
    if head:
        units.append(head)
    for opt in recipe.get("options", []):
        oid = opt["id"]
        raw = raw_opts.get(oid)
        if opt["kind"] == "toggle":
            value = resolve_option(opt, raw) if raw is not None else toggle_default(opt)
            chosen[oid] = value
            if value:
                unit = make_unit("%s.%s" % (rid, oid), opt["title"], opt)
                if unit:
                    units.append(unit)
            continue
        value = resolve_option(opt, raw) if raw is not None else option_default(opt)
        chosen[oid] = value
        picked = [value] if opt["kind"] == "single" else value
        for choice in opt["choices"]:
            if choice["id"] not in picked:
                continue
            unit = make_unit("%s.%s.%s" % (rid, oid, choice["id"]),
                             "%s: %s" % (opt["title"], choice["title"]), choice)
            if unit:
                units.append(unit)
    return units, chosen


def list_recipes(recipes_dir):
    out = []
    for path in sorted(Path(recipes_dir).glob("*.json")):
        rid = path.stem
        recipe = load_recipe(recipes_dir, rid)
        options = []
        for opt in recipe.get("options", []):
            entry = {"id": opt["id"], "title": opt["title"], "description": opt["description"],
                     "kind": opt["kind"], "default": option_default(opt)}
            if opt["kind"] != "toggle":
                entry["choices"] = [{"id": c["id"], "title": c["title"], "description": c.get("description", "")}
                                    for c in opt["choices"]]
            options.append(entry)
        out.append({"id": rid, "title": recipe["title"], "description": recipe["description"],
                    "always": rid == ALWAYS, "exclusive": recipe.get("exclusive"),
                    "session": recipe.get("session"), "options": options})
    return out


# ---------------------------------------------------------------- running steps

class Runner:
    def __init__(self, dry_run, assets_dir, log_path):
        self.dry_run = dry_run
        self.log_path = log_path
        self.env = dict(os.environ)
        self.env.update({"DEBIAN_FRONTEND": "noninteractive", "LC_ALL": "C.UTF-8",
                         "PM_SBX_ASSETS": assets_dir, "PM_SBX_GUEST": guest_dir_of(assets_dir)})

    def _append(self, text):
        os.makedirs(os.path.dirname(self.log_path), exist_ok=True)
        with open(self.log_path, "a", encoding="utf-8") as fh:
            fh.write(text + "\n")

    def exec(self, argv, shown, what):
        if self.dry_run:
            print("  + " + shown, flush=True)
            return
        self._append("+ " + shown)
        os.makedirs(os.path.dirname(self.log_path), exist_ok=True)
        with open(self.log_path, "a", encoding="utf-8") as fh:
            # stdin is closed so a tool that asks a question fails at once instead of hanging the install.
            code = subprocess.call(argv, env=self.env, stdin=subprocess.DEVNULL, stdout=fh, stderr=subprocess.STDOUT)
        if code != 0:
            raise StepError("%s failed (exit %d). Details: %s" % (what, code, self.log_path))

    def shell(self, line, what):
        self.exec(["/bin/bash", "-e", "-o", "pipefail", "-c", line], "sh: " + line, what)

    def apt_update(self):
        self.exec(["apt-get", "update"], "apt-get update", "apt update")

    def apt_install(self, packages, what):
        argv = ["apt-get", "install", "-y", "--no-install-recommends"] + list(packages)
        self.exec(argv, " ".join(shlex.quote(a) for a in argv), what)

    def fetch_deb(self, deb):
        url = deb["url"]
        name = os.path.basename(urllib.parse.urlsplit(url).path)
        if not DEB_NAME_RE.match(name):
            raise UsageError("cannot take a .deb name from %s" % url)
        dest = os.path.join(DEB_CACHE, name)
        pinned = deb.get("sha256")
        if self.dry_run:
            print("  + fetch %s -> %s (%s)" % (url, dest, "sha256 pinned" if pinned else "sha256 not pinned"),
                  flush=True)
            return dest
        os.makedirs(DEB_CACHE, exist_ok=True)
        if pinned and os.path.isfile(dest) and sha256_file(dest) == pinned:
            self._append("cached " + dest)
            return dest
        part = dest + ".part"
        digest = hashlib.sha256()
        size = 0
        try:
            with urllib.request.urlopen(url, timeout=120) as resp:
                if not resp.geturl().startswith("https://"):
                    raise StepError("download of %s was redirected to a non-https address" % name)
                with open(part, "wb") as out:
                    while True:
                        chunk = resp.read(1 << 16)
                        if not chunk:
                            break
                        size += len(chunk)
                        if size > MAX_DEB_BYTES:
                            raise StepError("%s is larger than the size limit" % name)
                        digest.update(chunk)
                        out.write(chunk)
        except OSError as err:
            raise StepError("download of %s failed: %s" % (url, err))
        got = digest.hexdigest()
        if pinned and got != pinned:
            os.unlink(part)
            raise StepError("checksum mismatch for %s" % name)
        if not pinned:
            self._append("UNPINNED %s sha256=%s size=%d" % (name, got, size))
        os.replace(part, dest)
        return dest


def write_desktop_conf(runner, session):
    body = ("# Written by apply-recipe.py from the chosen desktop recipe. Read by pm-sbx-start-desktop.\n"
            "PM_SBX_SESSION=%s\n" % session)
    if runner.dry_run:
        print("  + write %s: PM_SBX_SESSION=%s" % (DESKTOP_CONF, session), flush=True)
        return
    os.makedirs(os.path.dirname(DESKTOP_CONF), exist_ok=True)
    tmp = DESKTOP_CONF + ".tmp"
    Path(tmp).write_text(body, encoding="utf-8")
    os.replace(tmp, DESKTOP_CONF)


def run_unit(runner, unit):
    if "session" in unit:
        write_desktop_conf(runner, unit["session"])
        return
    for line in unit["pre"]:
        runner.shell(line, unit["title"])
    if unit["pre"]:
        # A pre line may have added an apt source; refresh the index before installing.
        runner.apt_update()
    if unit["apt"]:
        runner.apt_install(unit["apt"], unit["title"])
    for deb in unit["debs"]:
        path = runner.fetch_deb(deb)
        runner.exec(["apt-get", "install", "-y", "--no-install-recommends", path],
                    "apt-get install -y --no-install-recommends " + path, unit["title"])
    for line in unit["post"]:
        runner.shell(line, unit["title"])


# ---------------------------------------------------------------- state and preflight

def load_state(path):
    try:
        data = json.loads(Path(path).read_text(encoding="utf-8"))
    except FileNotFoundError:
        return {}
    except (OSError, ValueError):
        # A damaged state file only means steps run again. They are idempotent.
        return {}
    return data if isinstance(data, dict) else {}


def save_state(path, state):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    tmp = path + ".tmp"
    Path(tmp).write_text(json.dumps(state, indent=2, sort_keys=True), encoding="utf-8")
    os.replace(tmp, path)


def check_no_sudo(user):
    import pwd
    try:
        pwd.getpwnam(user)
    except KeyError:
        return  # base.json creates the user; nothing to check yet
    listed = subprocess.run(["id", "-nG", user], capture_output=True, text=True)
    groups = set(listed.stdout.split())
    bad = sorted(groups & {"sudo", "admin", "wheel"})
    if bad:
        raise PreflightError("user %s is in group(s) %s; the desktop user must have no sudo" % (user, ", ".join(bad)))
    for path in ["/etc/sudoers"] + sorted(glob.glob("/etc/sudoers.d/*")):
        try:
            text = Path(path).read_text(encoding="utf-8", errors="replace")
        except OSError:
            continue
        for line in text.splitlines():
            stripped = line.strip()
            if not stripped or stripped.startswith("#"):
                continue
            if stripped.split()[0] == user:
                raise PreflightError("%s grants %s rights in %s; remove that line" % (user, "sudo", path))


# ---------------------------------------------------------------- main

def parse_args(argv):
    ap = argparse.ArgumentParser(description="Install the PM sandbox desktop recipes (idempotent).")
    ap.add_argument("--recipes-dir", default=DEFAULT_RECIPES_DIR, help="folder with <recipe>.json files")
    ap.add_argument("--assets-dir", default=DEFAULT_ASSETS_DIR,
                    help="folder with the PM desktop files (a copy of guest/desktop); its parent folder must hold "
                         "egress/, control/ and page-browser/ (a copy of guest/)")
    ap.add_argument("--recipe", action="append", default=[], metavar="ID",
                    help="recipe to install; base is always included")
    ap.add_argument("--opt", action="append", default=[], metavar="RECIPE.OPTION[=VALUE]",
                    help="option value; a toggle needs no value. Naming a recipe here includes it.")
    ap.add_argument("--dry-run", action="store_true", help="print the plan and commands, change nothing")
    ap.add_argument("--list", action="store_true", help="print recipes and options as JSON and exit")
    ap.add_argument("--state-file", default=DEFAULT_STATE_FILE)
    ap.add_argument("--log-file", default=DEFAULT_LOG_FILE)
    return ap.parse_args(argv)


def plan_units(args):
    opts = parse_opts(args.opt)
    requested = [ALWAYS]
    for rid in list(args.recipe) + sorted(opts):
        if rid not in requested:
            requested.append(rid)
    units = []
    owners = {}
    session = None
    chosen_all = {}
    for rid in requested:
        recipe = load_recipe(args.recipes_dir, rid)
        exclusive = recipe.get("exclusive")
        if exclusive:
            if exclusive in owners:
                raise UsageError("%s and %s cannot both be installed (%s)" % (rid, owners[exclusive], exclusive))
            owners[exclusive] = rid
        rec_units, chosen = build_units(rid, recipe, opts.get(rid, {}))
        units.extend(rec_units)
        chosen_all[rid] = chosen
        if recipe.get("session"):
            session = recipe["session"]
    if session:
        units.append({"key": "desktop-conf", "title": "Desktop session (%s)" % session, "session": session,
                      "pre": [], "apt": [], "debs": [], "post": []})
    return requested, units, chosen_all


def run_plan(args, requested, units, chosen_all):
    total = len(units)
    runner = Runner(True, args.assets_dir, args.log_file)
    if any(u.get("apt") or u.get("debs") or u.get("pre") for u in units):
        print("  + apt-get update", flush=True)
    for number, unit in enumerate(units, 1):
        progress(number, total, "Would install " + unit["title"])
        for line in unit.get("pre", []):
            runner.shell(line, unit["title"])
        if unit.get("pre"):
            print("  + apt-get update", flush=True)
        if unit.get("apt"):
            runner.apt_install(unit["apt"], unit["title"])
        for deb in unit.get("debs", []):
            runner.fetch_deb(deb)
            print("  + apt-get install -y --no-install-recommends <downloaded .deb>", flush=True)
        for line in unit.get("post", []):
            runner.shell(line, unit["title"])
        if "session" in unit:
            write_desktop_conf(runner, unit["session"])
    emit_result({"status": "planned", "dry_run": True, "recipes": requested, "options": chosen_all,
                 "steps": [u["key"] for u in units]})
    return EXIT_OK


def run_install(args, requested, units, chosen_all):
    geteuid = getattr(os, "geteuid", None)
    if geteuid is None or geteuid() != 0:
        print("ERROR run as root inside the distro (python3 -I apply-recipe.py ...)", flush=True)
        return EXIT_ROOT
    if not Path(args.assets_dir).is_dir():
        raise UsageError("assets folder missing: %s (copy guest/desktop there first)" % args.assets_dir)
    try:
        check_no_sudo(DESKTOP_USER)
    except PreflightError as err:
        emit_error(err)
        return EXIT_SUDO
    adig = assets_digest(args.assets_dir)
    state = load_state(args.state_file)
    total = len(units)
    pending = [u for u in units if state.get(u["key"]) != unit_digest(u, adig)]
    runner = Runner(False, args.assets_dir, args.log_file)
    runner._append("== apply %s" % " ".join(requested))
    if any(u["apt"] or u["debs"] or u["pre"] for u in pending):
        progress(0, total, "Updating package lists")
        runner.apt_update()
    installed = []
    skipped = []
    for number, unit in enumerate(units, 1):
        digest = unit_digest(unit, adig)
        if state.get(unit["key"]) == digest:
            skipped.append(unit["key"])
            progress(number, total, "%s: already in place" % unit["title"])
            continue
        progress(number - 1, total, "Installing " + unit["title"])
        run_unit(runner, unit)
        state[unit["key"]] = digest
        save_state(args.state_file, state)
        installed.append(unit["key"])
        progress(number, total, "Installed " + unit["title"])
    emit_result({"status": "ok", "dry_run": False, "recipes": requested, "options": chosen_all,
                 "installed": installed, "skipped": skipped})
    return EXIT_OK


def main(argv=None):
    args = parse_args(argv)
    try:
        if args.list:
            print(json.dumps(list_recipes(args.recipes_dir), indent=2, sort_keys=True), flush=True)
            return EXIT_OK
        requested, units, chosen_all = plan_units(args)
        if args.dry_run:
            return run_plan(args, requested, units, chosen_all)
        return run_install(args, requested, units, chosen_all)
    except UsageError as err:
        emit_error(err)
        return EXIT_USAGE
    except StepError as err:
        emit_error(err)
        return EXIT_STEP


if __name__ == "__main__":
    sys.exit(main())

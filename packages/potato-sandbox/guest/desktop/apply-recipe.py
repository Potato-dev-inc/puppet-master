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
# In a real run <total> is 1000 and <n> is the share of the whole job that is done (a download that is half way
# is half way; see Reporter). A dry run counts steps instead.
#
# Speed (see also recipes/SCHEMA.md): the fastest nearby Ubuntu mirror is picked first (choose_mirror), the daily-update
# jobs are masked so nothing else holds the apt lock, packages are downloaded over several connections at once
# (Downloader) while Chrome downloads beside them, and dpkg runs with --force-unsafe-io.
# Exit codes: 0 ok, 1 a step failed, 2 bad recipe or option, 3 not root, 4 the desktop user has sudo.

import argparse
import glob
import hashlib
import http.client
import json
import os
import math
import re
import shlex
import ssl
import subprocess
import sys
import threading
import time
import urllib.parse
import urllib.request
from datetime import timedelta, timezone
from email.utils import parsedate_to_datetime
from pathlib import Path

DEFAULT_RECIPES_DIR = "/usr/local/share/pm-sbx/recipes"
DEFAULT_ASSETS_DIR = "/usr/local/share/pm-sbx/desktop"
# Folders next to the assets folder that post lines read through $PM_SBX_GUEST (the copy of the package's guest/ folder).
GUEST_SUBDIRS = ("egress", "control", "page-browser")
DEFAULT_STATE_FILE = "/var/lib/pm-sbx/recipes-state.json"
DEFAULT_LOG_FILE = "/var/log/pm-sbx/apply-recipe.log"
DESKTOP_CONF = "/etc/pm-sbx/desktop.conf"
DEB_CACHE = "/var/cache/pm-sbx/debs"
APT_ARCHIVES = "/var/cache/apt/archives"
APT_CONF_FILE = "/etc/apt/apt.conf.d/90pm-sbx"
APT_CONF = """// Written by apply-recipe.py (Puppet Master sandbox): faster, quieter apt.
Acquire::Languages "none";
Acquire::Retries "3";
Acquire::http::Pipeline-Depth "10";
APT::Install-Suggests "0";
DPkg::Lock::Timeout "180";
"""
SOURCES_FILE = "/etc/apt/sources.list.d/ubuntu.sources"
SOURCES_ORIG = "/var/lib/pm-sbx/ubuntu.sources.orig"
ARCHIVE_URL = "http://archive.ubuntu.com/ubuntu/"
SECURITY_URL = "http://security.ubuntu.com/ubuntu/"
MIRROR_LIST_URL = "http://mirrors.ubuntu.com/%s.txt"
MIRROR_BUDGET = 5.0          # seconds for the whole mirror pick
MAX_CANDIDATES = 10
DOWNLOAD_WORKERS = 8
# Services that would compete for the apt lock or the disk while we install. Masked (and stopped when systemd runs).
QUIET_UNITS = ("apt-daily.timer", "apt-daily-upgrade.timer", "apt-daily.service", "apt-daily-upgrade.service",
               "unattended-upgrades.service", "packagekit.service", "packagekit-offline-update.service")
# Options on every apt-get install. unsafe-io: no fsync per file (a failed install is thrown away and redone anyway).
APT_RUN_OPTS = ["-o", "Dpkg::Options::=--force-confdef", "-o", "Dpkg::Options::=--force-confold",
                "-o", "Dpkg::Options::=--force-unsafe-io", "-o", "Dpkg::Progress-Fancy=0", "-o", "APT::Color=0"]
ALWAYS = "base"
DESKTOP_USER = "pm"
SESSIONS = ("fluxbox", "xfce4")
MAX_DEB_BYTES = 600 * 1024 * 1024
BLOCK_KEYS = ("pre", "apt", "debs", "post")
META_KEYS = ("size_mb", "early_pre")

ID_RE = re.compile(r"^[a-z][a-z0-9-]{0,47}$")
PKG_RE = re.compile(r"^[a-z0-9][a-z0-9.+-]{0,127}$")
SHA_RE = re.compile(r"^[0-9a-f]{64}$")
URI_LINE_RE = re.compile(r"^'(https?://[^']+)'\s+(\S+)\s+(\d+)\s*(\S*)")
ARCHIVE_NAME_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._+~%-]*\.deb$")
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
    # size_mb only steers the progress bar and early_pre only the order of work; changing them must not make a finished
    # step run again.
    shown = {k: v for k, v in unit.items() if k not in META_KEYS}
    blob = json.dumps({"unit": shown, "assets": assets_hash}, sort_keys=True).encode("utf-8")
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
    if block.get("early_pre") is not None and not isinstance(block["early_pre"], bool):
        raise UsageError("%s.early_pre: must be true or false" % where)
    size = block.get("size_mb")
    if size is not None and (isinstance(size, bool) or not isinstance(size, (int, float)) or size <= 0):
        raise UsageError("%s.size_mb: must be a positive number (the estimated download, for the progress bar)" % where)
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
    requires = data.get("requires")
    if requires is not None and (not isinstance(requires, str) or not ID_RE.match(requires)):
        raise UsageError("%s.requires: must be a recipe id or null" % rid)
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
    if isinstance(block.get("size_mb"), (int, float)) and not isinstance(block.get("size_mb"), bool):
        unit["size_mb"] = block["size_mb"]
    if block.get("early_pre") is True:
        unit["early_pre"] = True
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
                    "session": recipe.get("session"), "requires": recipe.get("requires"), "options": options})
    return out


# ---------------------------------------------------------------- progress

def fmt_mb(nbytes):
    mb = nbytes / 1048576.0
    return "%d" % round(mb) if mb >= 10 else "%.1f" % mb


def fmt_wait(seconds):
    seconds = int(seconds)
    if seconds < 90:
        return "%d s" % max(seconds, 1)
    return "%d min" % round(seconds / 60.0)


class Reporter:
    """Turns "unit k is x done" into one steady line: PROGRESS <n>/1000 <text>.

    The units are weighted (their estimated size), so a big download moves the bar as much as it should. The number
    never goes down and never reaches 1000 before finish()."""

    SCALE = 1000
    MIN_GAP = 0.45  # seconds between two lines, unless forced

    def __init__(self, weights, clock=time.monotonic):
        total = float(sum(weights)) or 1.0
        self.starts = []
        self.spans = []
        acc = 0.0
        for weight in weights:
            self.starts.append(acc / total)
            self.spans.append(weight / total)
            acc += weight
        self.index = 0
        self.last = 0
        self.last_text = None
        self.last_at = None
        self.clock = clock

    def begin(self, index):
        self.index = index

    def update(self, fraction, text, force=False):
        fraction = min(max(float(fraction), 0.0), 1.0)
        overall = self.starts[self.index] + fraction * self.spans[self.index]
        number = max(self.last, min(int(overall * self.SCALE), self.SCALE - 1))
        now = self.clock()
        if number == self.last and text == self.last_text:
            return
        if not force and self.last_at is not None and now - self.last_at < self.MIN_GAP:
            return
        self.last, self.last_text, self.last_at = number, text, now
        print("PROGRESS %d/%d %s" % (number, self.SCALE, " ".join(str(text).split())), flush=True)

    def finish(self, text):
        self.last = self.SCALE
        print("PROGRESS %d/%d %s" % (self.SCALE, self.SCALE, " ".join(str(text).split())), flush=True)


class UnitProgress:
    """The stages inside one unit and how much of the unit each is worth."""

    def __init__(self, reporter, title, stages):
        self.reporter = reporter
        self.title = title
        self.stages = stages
        self.total = float(sum(w for _, w in stages)) or 1.0

    def at(self, stage, fraction, what, force=False):
        before = 0.0
        weight = 0.0
        for name, w in self.stages:
            if name == stage:
                weight = w
                break
            before += w
        fraction = min(max(float(fraction), 0.0), 1.0)
        self.reporter.update((before + weight * fraction) / self.total, "%s: %s" % (self.title, what), force)


def unit_stages(unit):
    stages = []
    if unit["pre"]:
        stages.append(("pre", 2.0 * len(unit["pre"]) + 3.0))
    if unit["apt"] or unit["debs"]:
        stages.append(("download", 45.0))
        stages.append(("install", 30.0))
    if unit["post"]:
        stages.append(("post", min(6.0, 0.4 * len(unit["post"]))))
    return stages or [("work", 1.0)]


def unit_weight(unit):
    """Estimated size in MB, for the bar only. A recipe can say size_mb; else a guess from what it installs."""
    declared = unit.get("size_mb")
    if isinstance(declared, (int, float)) and declared > 0:
        return float(declared)
    return 4.0 + 3.0 * len(unit["apt"]) + 20.0 * len(unit["debs"]) + 0.5 * len(unit["post"])


# ---------------------------------------------------------------- the fastest mirror

def local_country():
    """Two-letter country of the PC's time zone (WSL copies the Windows zone), or None."""
    zone = None
    try:
        link = os.readlink("/etc/localtime")
        if "zoneinfo/" in link:
            zone = link.split("zoneinfo/", 1)[1]
    except OSError:
        pass
    if not zone:
        try:
            zone = Path("/etc/timezone").read_text(encoding="utf-8").strip() or None
        except OSError:
            zone = None
    if not zone:
        return None
    for tab in ("/usr/share/zoneinfo/zone.tab", "/usr/share/zoneinfo/zone1970.tab"):
        try:
            lines = Path(tab).read_text(encoding="utf-8").splitlines()
        except OSError:
            continue
        for line in lines:
            parts = line.split("\t")
            if len(parts) >= 3 and parts[2].strip() == zone:
                code = parts[0].split(",")[0].strip().upper()
                if re.match(r"^[A-Z]{2}$", code):
                    return code
    return None


def parse_mirror_list(text):
    """Mirror base URLs from mirrors.ubuntu.com text: http(s) only, amd64 archives only (no ports), one slash at the end."""
    out = []
    for line in text.splitlines():
        url = line.strip()
        if not re.match(r"^https?://[A-Za-z0-9.-]+(:\d+)?/", url) or "ports" in url or " " in url:
            continue
        if not url.endswith("/"):
            url += "/"
        if url not in out:
            out.append(url)
    return out


def release_date(text):
    """The Date: line of an InRelease file, as an aware datetime, or None."""
    match = re.search(r"^Date:\s*(.+)$", text, re.MULTILINE)
    if not match:
        return None
    try:
        when = parsedate_to_datetime(match.group(1).strip())
    except (TypeError, ValueError):
        return None
    if when.tzinfo is None:
        when = when.replace(tzinfo=timezone.utc)
    return when


def choose_best(stage1, stage2):
    """stage1: [{base, latency, date}]; stage2: {base: seconds for a 400 KB read}. Returns (best, why) or (None, why).

    A mirror that is more than a day behind the newest one is dropped (stale lists break installs). Then the one that
    read the sample fastest wins; mirrors without a sample are ranked by the first answer's speed."""
    live = [r for r in stage1 if r.get("date") is not None]
    if not live:
        return None, "no mirror answered"
    newest = max(r["date"] for r in live)
    fresh = [r for r in live if newest - r["date"] <= timedelta(hours=24)]

    def key(r):
        sample = stage2.get(r["base"])
        return (0 if sample is not None else 1, sample if sample is not None else r["latency"], r["latency"])

    fresh.sort(key=key)
    return fresh[0], "ok"


def http_read(url, limit, timeout):
    """(bytes read up to limit, seconds)."""
    started = time.monotonic()
    request = urllib.request.Request(url, headers={"User-Agent": "pm-sbx-apply"})
    with urllib.request.urlopen(request, timeout=timeout) as resp:
        data = resp.read(limit)
    return data, time.monotonic() - started


def run_parallel(jobs, timeout):
    """Runs the callables in daemon threads and returns what finished within timeout seconds (a slow one is left behind)."""
    results = []
    lock = threading.Lock()

    def wrap(job):
        try:
            value = job()
        except Exception:  # a mirror that fails is just not a candidate
            return
        with lock:
            results.append(value)

    threads = [threading.Thread(target=wrap, args=(job,), daemon=True) for job in jobs]
    for thread in threads:
        thread.start()
    end = time.monotonic() + timeout
    for thread in threads:
        thread.join(max(0.0, end - time.monotonic()))
    with lock:
        return list(results)


def probe_first(base, codename):
    def job():
        data, took = http_read(base + "dists/%s-updates/InRelease" % codename, 400000, 2.5)
        text = data.decode("utf-8", "replace")
        if "Suite: %s-updates" % codename not in text:
            raise ValueError("not an Ubuntu archive")
        return {"base": base, "latency": took, "date": release_date(text)}
    return job


def carries_security(base, codename):
    try:
        data, _ = http_read(base + "dists/%s-security/InRelease" % codename, 4096, 1.5)
        return "Suite: %s-security" % codename in data.decode("utf-8", "replace")
    except Exception:
        return False


def probe_sample(base, codename):
    def job():
        data, took = http_read(base + "dists/%s/main/binary-amd64/Packages.xz" % codename, 400000, 2.0)
        if len(data) < 100000:
            raise ValueError("short read")
        return (base, took)
    return job


def rewrite_sources(text, codename, mirror, security):
    """Points the Ubuntu stanzas of a deb822 sources file at the mirror (and the security stanza at `security`)."""
    stanzas = re.split(r"\n\s*\n", text.strip("\n"))
    out = []
    for stanza in stanzas:
        lines = stanza.split("\n")
        suites = next((l.split(":", 1)[1].split() for l in lines if l.startswith("Suites:")), [])
        is_ubuntu = any(s == codename or s.startswith(codename + "-") for s in suites)
        if is_ubuntu and any(l.startswith("URIs:") for l in lines):
            target = security if any(s.endswith("-security") for s in suites) else mirror
            lines = ["URIs: " + target if l.startswith("URIs:") else l for l in lines]
        out.append("\n".join(lines))
    return "\n\n".join(out) + "\n"


def os_codename():
    try:
        for line in Path("/etc/os-release").read_text(encoding="utf-8").splitlines():
            if line.startswith("VERSION_CODENAME="):
                return line.split("=", 1)[1].strip().strip('"') or "noble"
    except OSError:
        pass
    return "noble"


def choose_mirror(log):
    """Picks the fastest nearby Ubuntu mirror inside MIRROR_BUDGET seconds and writes it into the sources file.
    Returns a short text for the progress line and the mirror (or None when the archive stays)."""
    started = time.monotonic()
    codename = os_codename()
    try:
        original = Path(SOURCES_FILE).read_text(encoding="utf-8")
    except OSError:
        log("mirror: no %s; leaving apt alone" % SOURCES_FILE)
        return "default Ubuntu server", None
    country = local_country()
    lists = [MIRROR_LIST_URL % "mirrors"] + ([MIRROR_LIST_URL % country] if country else [])

    def fetch_list(url):
        def job():
            data, _ = http_read(url, 65536, 2.0)
            return parse_mirror_list(data.decode("utf-8", "replace"))
        return job

    candidates = []
    for found in run_parallel([fetch_list(u) for u in lists], 2.5):
        for url in found:
            if url not in candidates:
                candidates.append(url)
    if ARCHIVE_URL not in candidates:
        candidates.append(ARCHIVE_URL)
    candidates = [c for c in candidates if c != ARCHIVE_URL][:MAX_CANDIDATES - 1] + [ARCHIVE_URL]
    stage1 = run_parallel([probe_first(c, codename) for c in candidates],
                          max(0.5, min(3.2, MIRROR_BUDGET - (time.monotonic() - started) - 1.8)))
    top = sorted((r for r in stage1 if r.get("date") is not None), key=lambda r: r["latency"])[:3]
    sample_time = max(0.5, MIRROR_BUDGET - (time.monotonic() - started))
    stage2 = dict(run_parallel([probe_sample(r["base"], codename) for r in top], sample_time))
    best, why = choose_best(stage1, stage2)
    spent = time.monotonic() - started
    log("mirror: %d candidates (country %s), %d answered, %s in %.1f s" % (len(candidates), country, len(stage1), why, spent))
    for row in sorted(stage1, key=lambda r: r["latency"]):
        log("mirror:   %s first=%.2fs sample=%s" % (
            row["base"], row["latency"], ("%.2fs" % stage2[row["base"]]) if row["base"] in stage2 else "-"))
    if best is None:
        return "default Ubuntu server", None
    mirror = best["base"]
    security = mirror if carries_security(mirror, codename) else SECURITY_URL
    if not Path(SOURCES_ORIG).exists():
        os.makedirs(os.path.dirname(SOURCES_ORIG), exist_ok=True)
        Path(SOURCES_ORIG).write_text(original, encoding="utf-8")
    updated = rewrite_sources(original, codename, mirror, security)
    if updated != original:
        tmp = SOURCES_FILE + ".pm-tmp"
        Path(tmp).write_text(updated, encoding="utf-8")
        os.replace(tmp, SOURCES_FILE)
    log("mirror: using %s (security: %s)" % (mirror, security))
    return urllib.parse.urlsplit(mirror).netloc, mirror


# ---------------------------------------------------------------- parallel downloads

class FetchTask:
    """One download. size is what the progress bar expects (bytes); run(on_bytes) does it and raises on failure."""

    def __init__(self, name, size, run, required):
        self.name = name
        self.size = size
        self.run = run
        self.required = required
        self.error = None


class Fetcher:
    """Several downloads at once, biggest first. Watch it with progress(); wait with finished()."""

    def __init__(self, tasks, workers=DOWNLOAD_WORKERS, clock=time.monotonic):
        self.tasks = sorted(tasks, key=lambda t: -t.size)
        self.total = sum(t.size for t in self.tasks)
        self.done = 0
        self.lock = threading.Lock()
        self.queue = list(self.tasks)
        self.clock = clock
        self.window = []
        self.threads = [threading.Thread(target=self._work, daemon=True) for _ in range(min(workers, len(self.tasks)))]

    def start(self):
        for thread in self.threads:
            thread.start()

    def _on_bytes(self, count):
        with self.lock:
            self.done += count

    def _work(self):
        while True:
            with self.lock:
                if not self.queue:
                    return
                task = self.queue.pop(0)
            try:
                task.run(self._on_bytes)
            except Exception as err:  # reported through task.error; the caller decides if it is fatal
                task.error = str(err) or err.__class__.__name__

    def finished(self):
        return not any(thread.is_alive() for thread in self.threads)

    def wait(self, tick, interval=0.4):
        while not self.finished():
            time.sleep(interval)
            tick(*self.progress())
        tick(*self.progress())

    def progress(self):
        """(bytes done, bytes expected, bytes per second over the last 3 s)."""
        now = self.clock()
        with self.lock:
            done = self.done
        self.window.append((now, done))
        while len(self.window) > 2 and now - self.window[0][0] > 3.0:
            self.window.pop(0)
        first_at, first_done = self.window[0]
        span = now - first_at
        speed = (done - first_done) / span if span > 0.2 else 0.0
        return done, self.total, speed


def wait_fetchers(fetchers, tick, interval=0.4):
    """Waits for several Fetchers and tells tick(done, total, speed) with their sums."""
    def sums():
        rows = [f.progress() for f in fetchers]
        return sum(r[0] for r in rows), sum(r[1] for r in rows), sum(r[2] for r in rows)

    while not all(f.finished() for f in fetchers):
        time.sleep(interval)
        tick(*sums())
    tick(*sums())


def download_line(done, total, speed):
    """'Downloading 120 of 340 MB · 4.1 MB/s · about 1 min left'"""
    parts = ["Downloading %s of %s MB" % (fmt_mb(done), fmt_mb(total)) if total > 0 else "Downloading"]
    if speed > 50 * 1024:
        parts.append("%.1f MB/s" % (speed / 1048576.0))
        left = max(total - done, 0) / speed
        if total > 0 and done > 0 and done < total:
            parts.append("about %s left" % fmt_wait(left))
    return " · ".join(parts)


def parse_print_uris(text):
    """Items from `apt-get install --print-uris -qq`: [{url, name, size, sha256}]."""
    items = []
    for line in text.splitlines():
        match = URI_LINE_RE.match(line.strip())
        if not match:
            continue
        url, name, size, checksum = match.groups()
        if not ARCHIVE_NAME_RE.match(name):
            continue
        sha = checksum.split(":", 1)[1].lower() if checksum.upper().startswith("SHA256:") else None
        if sha is not None and not SHA_RE.match(sha):
            sha = None
        items.append({"url": url, "name": name, "size": int(size), "sha256": sha})
    return items


def count_installs(text):
    """Packages apt would install, from `apt-get -s install` (the 'Inst' lines)."""
    return sum(1 for line in text.splitlines() if line.startswith("Inst "))


class Connections:
    """Keep-alive connections, one per host, for one worker thread."""

    def __init__(self):
        self.by_host = {}
        self.context = ssl.create_default_context()

    def get(self, url):
        parts = urllib.parse.urlsplit(url)
        key = (parts.scheme, parts.netloc)
        conn = self.by_host.get(key)
        if conn is None:
            if parts.scheme == "https":
                conn = http.client.HTTPSConnection(parts.netloc, timeout=30, context=self.context)
            else:
                conn = http.client.HTTPConnection(parts.netloc, timeout=30)
            self.by_host[key] = conn
        path = parts.path + (("?" + parts.query) if parts.query else "")
        return key, conn, path

    def drop(self, key):
        conn = self.by_host.pop(key, None)
        if conn is not None:
            try:
                conn.close()
            except OSError:
                pass


_local = threading.local()


def archive_task(item):
    """Downloads one .deb into apt's archive folder, checked, so apt-get install finds it already there."""
    dest = os.path.join(APT_ARCHIVES, item["name"])
    part = os.path.join(APT_ARCHIVES, "partial", item["name"] + ".pm")

    def run(on_bytes):
        if os.path.isfile(dest) and os.path.getsize(dest) == item["size"]:
            on_bytes(item["size"])
            return
        conns = getattr(_local, "conns", None)
        if conns is None:
            conns = _local.conns = Connections()
        last = None
        for _attempt in range(3):
            counted = 0
            key = None
            try:
                key, conn, path = conns.get(item["url"])
                conn.request("GET", path, headers={"User-Agent": "pm-sbx-apply", "Accept-Encoding": "identity"})
                resp = conn.getresponse()
                if resp.status != 200:
                    resp.read()
                    raise OSError("HTTP %d" % resp.status)
                digest = hashlib.sha256()
                size = 0
                os.makedirs(os.path.dirname(part), exist_ok=True)
                with open(part, "wb") as out:
                    while True:
                        chunk = resp.read(262144)
                        if not chunk:
                            break
                        out.write(chunk)
                        digest.update(chunk)
                        size += len(chunk)
                        counted += len(chunk)
                        on_bytes(len(chunk))
                if size != item["size"]:
                    raise OSError("size %d, expected %d" % (size, item["size"]))
                if item["sha256"] and digest.hexdigest() != item["sha256"]:
                    raise OSError("checksum mismatch")
                os.replace(part, dest)
                return
            except (OSError, http.client.HTTPException) as err:
                last = err
                on_bytes(-counted)
                if key is not None:
                    conns.drop(key)
                try:
                    os.unlink(part)
                except OSError:
                    pass
        raise StepError("download of %s failed: %s" % (item["name"], last))

    return FetchTask(item["name"], item["size"], run, required=False)


# ---------------------------------------------------------------- look ahead

def lookahead_candidate(upcoming):
    """The next unit whose downloads may start while the current one installs, or None.

    Units that only run post lines are passed over. The first unit that installs packages qualifies when it has no pre lines
    (it needs nothing from the units before it) or its recipe says early_pre (its pre lines need only what a stock Ubuntu
    has). Otherwise nothing after it may start early either: it might depend on what is installed by then."""
    for unit in upcoming:
        if "session" in unit or not (unit["apt"] or unit["debs"]):
            continue
        if unit["pre"] and not unit.get("early_pre"):
            return None
        return unit
    return None


class Lookahead:
    """The part of a later unit that does not need the units before it installed: its pre lines (early_pre), the list refresh,
    the plan and the downloads. It runs on a thread while the current unit unpacks, so the network is not idle during installs.
    Any failure is only noted: the unit then does its own work as usual, with what is already downloaded kept."""

    def __init__(self, runner, unit, after):
        self.runner = runner
        self.unit = unit
        self.after = after          # the units behind this one, for the next look ahead
        self.fetchers = []
        self.count = None
        self.pre_done = False
        self.error = None
        self.done = threading.Event()
        self.thread = threading.Thread(target=self._run, daemon=True)

    def _run(self):
        runner, unit = self.runner, self.unit
        try:
            if unit["pre"]:
                for line in unit["pre"]:
                    runner.shell(line, unit["title"])
                runner.apt_update()
                self.pre_done = True
            if unit["apt"]:
                self.count, items = runner.plan_install(unit["apt"])
                if items:
                    fetcher = Fetcher([archive_task(item) for item in items])
                    self.fetchers.append(fetcher)
                    fetcher.start()
                    while not fetcher.finished():
                        time.sleep(0.2)
            # Behind it, the next one can go while this one still waits for its turn to install.
            runner.start_lookahead(self.after)
        except (StepError, UsageError, OSError) as err:
            self.error = str(err) or err.__class__.__name__
            runner._append("look ahead for %s failed (%s); it will do its own work" % (unit["key"], self.error))
        finally:
            self.done.set()

    def start(self):
        self.thread.start()


# ---------------------------------------------------------------- running steps

class Runner:
    def __init__(self, dry_run, assets_dir, log_path):
        self.dry_run = dry_run
        self.log_path = log_path
        self.env = dict(os.environ)
        self.env.update({"DEBIAN_FRONTEND": "noninteractive", "LC_ALL": "C.UTF-8",
                         "NEEDRESTART_MODE": "a", "NEEDRESTART_SUSPEND": "1", "APT_LISTCHANGES_FRONTEND": "none",
                         "PM_SBX_ASSETS": assets_dir, "PM_SBX_GUEST": guest_dir_of(assets_dir)})
        self.mirror = None
        self.downloaded = 0
        self.early = {}  # deb url -> (FetchTask, Fetcher, path): started before the first unit so Chrome is ready sooner
        self.looking = {}  # unit key -> Lookahead
        self.look_lock = threading.Lock()

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

    def stream(self, argv, shown, what, on_line=None, on_tick=None, tick=1.0):
        """Like exec, but reads the output line by line (it still goes to the log) and calls on_tick about once a second."""
        if self.dry_run:
            print("  + " + shown, flush=True)
            return
        import queue
        self._append("+ " + shown)
        proc = subprocess.Popen(argv, env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True, encoding="utf-8", errors="replace")
        lines = queue.Queue()

        def reader():
            for line in proc.stdout:
                lines.put(line)
            lines.put(None)

        threading.Thread(target=reader, daemon=True).start()
        with open(self.log_path, "a", encoding="utf-8") as fh:
            while True:
                try:
                    line = lines.get(timeout=tick)
                except queue.Empty:
                    if on_tick:
                        on_tick()
                    continue
                if line is None:
                    break
                fh.write(line)
                if on_line:
                    on_line(line.rstrip("\r\n"))
                if on_tick:
                    on_tick()
        code = proc.wait()
        if code != 0:
            raise StepError("%s failed (exit %d). Details: %s" % (what, code, self.log_path))

    def shell(self, line, what):
        self.exec(["/bin/bash", "-e", "-o", "pipefail", "-c", line], "sh: " + line, what)

    # ---- before the first apt call

    def quiet_guest(self):
        """Nothing else may use apt, dpkg or the disk while we install: stop and mask the update jobs."""
        if self.dry_run:
            print("  + mask packagekit, unattended-upgrades and the apt-daily timers", flush=True)
            return
        systemd_up = os.path.isdir("/run/systemd/system")
        for unit in QUIET_UNITS:
            if systemd_up:
                try:
                    subprocess.run(["systemctl", "stop", unit], timeout=25, stdin=subprocess.DEVNULL,
                                   stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                except (OSError, subprocess.SubprocessError):
                    pass
            link = "/etc/systemd/system/" + unit
            if not os.path.lexists(link):
                try:
                    os.makedirs("/etc/systemd/system", exist_ok=True)
                    os.symlink("/dev/null", link)
                except OSError:
                    pass
        self._append("quiet: update jobs masked (systemd %s)" % ("running, stopped them" if systemd_up else "off"))

    def write_apt_conf(self):
        if self.dry_run:
            print("  + write %s (no language files, retries, pipelining, lock wait)" % APT_CONF_FILE, flush=True)
            return
        os.makedirs(os.path.dirname(APT_CONF_FILE), exist_ok=True)
        Path(APT_CONF_FILE).write_text(APT_CONF, encoding="utf-8")

    def pick_mirror(self):
        if self.dry_run:
            print("  + choose the fastest nearby Ubuntu mirror (mirrors.ubuntu.com list + archive.ubuntu.com)", flush=True)
            return "default Ubuntu server"
        name, mirror = choose_mirror(self._append)
        self.mirror = mirror
        return name

    # ---- apt

    def fall_back_to_archive(self):
        """The mirror we picked failed a real apt call: put the original sources back. True when that changed anything."""
        if not self.mirror or not os.path.isfile(SOURCES_ORIG):
            return False
        self._append("mirror %s failed an apt call; going back to the Ubuntu archive" % self.mirror)
        tmp = SOURCES_FILE + ".pm-tmp"
        Path(tmp).write_text(Path(SOURCES_ORIG).read_text(encoding="utf-8"), encoding="utf-8")
        os.replace(tmp, SOURCES_FILE)
        self.mirror = None
        return True

    def apt_update(self, tell=None):
        started = time.monotonic()

        def tick():
            if tell:
                tell(1.0 - math.exp(-(time.monotonic() - started) / 10.0), "Updating package lists")

        argv = ["apt-get", "update", "-o", "APT::Color=0"]
        try:
            self.stream(argv, "apt-get update", "apt update", on_tick=tick)
        except StepError:
            if not self.fall_back_to_archive():
                raise
            self.stream(argv, "apt-get update", "apt update", on_tick=tick)

    def plan_install(self, packages, uris=True):
        """(how many packages apt would install or None, [items to download]). Never raises: a plan is only a help."""
        base = ["apt-get", "install", "-y", "--no-install-recommends"] + APT_RUN_OPTS
        count = None
        items = []
        try:
            sim = subprocess.run(base[:1] + ["-s"] + base[1:] + list(packages), env=self.env, stdin=subprocess.DEVNULL,
                                 capture_output=True, text=True, errors="replace", timeout=120)
            if sim.returncode == 0:
                count = count_installs(sim.stdout)
            if uris:
                listed = subprocess.run(base + ["--print-uris", "-qq"] + list(packages), env=self.env,
                                        stdin=subprocess.DEVNULL, capture_output=True, text=True, errors="replace",
                                        timeout=120)
                if listed.returncode == 0:
                    items = parse_print_uris(listed.stdout)
        except (OSError, subprocess.SubprocessError) as err:
            self._append("plan: %s" % err)
        return count, items

    def apt_install(self, packages, what, count=None, tell=None):
        """apt-get install with the packages; `tell(fraction, text)` hears about the unpacking and setting up."""
        argv = ["apt-get", "install", "-y", "--no-install-recommends"] + APT_RUN_OPTS + list(packages)
        shown = " ".join(shlex.quote(a) for a in ["apt-get", "install", "-y", "--no-install-recommends"] + list(packages))
        if self.dry_run:
            print("  + " + shown, flush=True)
            return
        state = {"unpacked": 0, "setup": 0, "last": None, "started": time.monotonic()}

        def on_line(line):
            if line.startswith("Unpacking "):
                state["unpacked"] += 1
                state["last"] = line.split()[1] if len(line.split()) > 1 else None
            elif line.startswith("Setting up "):
                state["setup"] += 1
                state["last"] = line.split()[2] if len(line.split()) > 2 else None
            report()

        def report():
            if not tell:
                return
            if count:
                total = float(count)
                fraction = min(1.0, (state["unpacked"] + state["setup"]) / (2.0 * total))
                if state["unpacked"] < count:
                    text = "Unpacking %d of %d" % (min(state["unpacked"] + 1, count), count)
                else:
                    text = "Setting up %d of %d" % (min(state["setup"] + 1, count), count)
                if state["last"]:
                    text += " · " + state["last"].split(":")[0]
            else:
                elapsed = time.monotonic() - state["started"]
                fraction = 1.0 - math.exp(-elapsed / 60.0)
                text = "Installing packages"
            tell(fraction, text)

        self.stream(argv, shown, what, on_line=on_line, on_tick=report)

    def start_lookahead(self, upcoming):
        """Starts the look ahead for the first unit of `upcoming` that qualifies (see lookahead_candidate)."""
        if self.dry_run:
            return
        unit = lookahead_candidate(upcoming)
        if unit is None:
            return
        with self.look_lock:
            if unit["key"] in self.looking:
                return
            rest = upcoming[upcoming.index(unit) + 1:]
            look = Lookahead(self, unit, rest)
            self.looking[unit["key"]] = look
        look.start()

    def start_early_debs(self, units):
        """Starts the recipe .deb downloads now; they only need the network, not apt. run_unit picks them up."""
        for unit in units:
            for deb in unit.get("debs", []):
                if deb["url"] in self.early:
                    continue
                try:
                    task, path = self.fetch_deb_task(deb)
                except UsageError:
                    continue
                if task is None:
                    continue
                fetcher = Fetcher([task], workers=1)
                fetcher.start()
                self.early[deb["url"]] = (task, fetcher, path)

    def fetch_deb_task(self, deb):
        """A FetchTask for a recipe .deb (https only, sha256 checked when pinned), or None when it is already cached."""
        url = deb["url"]
        name = os.path.basename(urllib.parse.urlsplit(url).path)
        if not DEB_NAME_RE.match(name):
            raise UsageError("cannot take a .deb name from %s" % url)
        dest = os.path.join(DEB_CACHE, name)
        pinned = deb.get("sha256")
        if pinned and os.path.isfile(dest) and sha256_file(dest) == pinned:
            self._append("cached " + dest)
            return None, dest
        size = 150 * 1048576
        try:
            request = urllib.request.Request(url, method="HEAD", headers={"User-Agent": "pm-sbx-apply"})
            with urllib.request.urlopen(request, timeout=15) as resp:
                size = int(resp.headers.get("Content-Length") or size)
        except (OSError, ValueError):
            pass

        def run(on_bytes):
            self.fetch_deb(deb, on_bytes)

        return FetchTask(name, size, run, required=True), dest

    def fetch_deb(self, deb, on_bytes=None):
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
            request = urllib.request.Request(url, headers={"User-Agent": "pm-sbx-apply"})
            with urllib.request.urlopen(request, timeout=120) as resp:
                if not resp.geturl().startswith("https://"):
                    raise StepError("download of %s was redirected to a non-https address" % name)
                with open(part, "wb") as out:
                    while True:
                        chunk = resp.read(1 << 18)
                        if not chunk:
                            break
                        size += len(chunk)
                        if size > MAX_DEB_BYTES:
                            raise StepError("%s is larger than the size limit" % name)
                        digest.update(chunk)
                        out.write(chunk)
                        if on_bytes:
                            on_bytes(len(chunk))
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


def run_unit(runner, unit, up=None, upcoming=()):
    """Runs one unit. `up` is a UnitProgress (None in a dry run); `upcoming` are the units after this one."""
    if "session" in unit:
        write_desktop_conf(runner, unit["session"])
        return
    say = (lambda stage, fraction, what, force=False: up.at(stage, fraction, what, force)) if up else (lambda *a, **k: None)
    with runner.look_lock:
        look = runner.looking.pop(unit["key"], None)
    if look is not None:
        # This unit's downloads (and pre lines) started while the one before it was installing: take them over.
        while not look.done.is_set():
            if look.fetchers:
                rows = [f.progress() for f in look.fetchers]
                done, total = sum(r[0] for r in rows), sum(r[1] for r in rows)
                say("download", done / total if total else 0.0, download_line(done, total, sum(r[2] for r in rows)))
            else:
                say("pre", 0.5, "Getting ready")
            look.done.wait(0.4)
        if look.error:
            look = None
    pre = unit["pre"]
    if not (look and look.pre_done):
        for number, line in enumerate(pre):
            say("pre", 2.0 * number / (len(pre) * 2.0 + 3.0), "Preparing", True)
            runner.shell(line, unit["title"])
        if pre:
            # A pre line may have added an apt source; refresh the index before installing.
            runner.apt_update(lambda f, t: say("pre", (len(pre) * 2.0 + 3.0 * f) / (len(pre) * 2.0 + 3.0), t))
    if unit["apt"] or unit["debs"]:
        # 1) download: the packages over several connections at once, the .deb files beside them
        say("download", 0.0, "Looking up what to download", True)
        if look and look.fetchers:
            runner.downloaded += sum(max(f.done, 0) for f in look.fetchers)
            count, items = look.count, []
        else:
            count, items = runner.plan_install(unit["apt"]) if unit["apt"] else (None, [])
        tasks = [archive_task(item) for item in items]
        deb_paths = []
        watching = []
        for deb in unit["debs"]:
            early = runner.early.pop(deb["url"], None)
            if early:
                task, fetcher, path = early
                watching.append((task, fetcher))
                deb_paths.append(path)
                continue
            task, path = runner.fetch_deb_task(deb)
            deb_paths.append(path)
            if task:
                tasks.append(task)
        fetchers = [fetcher for _, fetcher in watching]
        if tasks:
            fetcher = Fetcher(tasks)
            fetcher.start()
            fetchers.append(fetcher)
            watching.extend((task, fetcher) for task in tasks)
        if fetchers:
            wait_fetchers(fetchers, lambda done, total, speed: say("download", done / total if total else 0.0,
                                                                  download_line(done, total, speed)))
            runner.downloaded += sum(max(f.done, 0) for f in fetchers)
            for task, _ in watching:
                if task.error and task.required:
                    raise StepError(task.error)
                if task.error:
                    runner._append("prefetch of %s failed (%s); apt will fetch it" % (task.name, task.error))
        # 2) install everything in one apt run (one set of triggers: fonts, ldconfig, icons); meanwhile the next unit downloads
        say("install", 0.0, "Installing packages", True)
        runner.start_lookahead(list(upcoming))
        everything = list(unit["apt"]) + deb_paths
        count_all, rest = runner.plan_install(everything)
        if rest:  # what the .deb files need that the first look did not know about (small)
            leftovers = Fetcher([archive_task(item) for item in rest])
            leftovers.start()
            wait_fetchers([leftovers], lambda done, total, speed: say("install", 0.0, download_line(done, total, speed)))
            runner.downloaded += max(leftovers.done, 0)
        runner.apt_install(everything, unit["title"], count_all if count_all is not None else count,
                           lambda f, t: say("install", f, t))
    for number, line in enumerate(unit["post"]):
        say("post", number / max(len(unit["post"]), 1), "Setting up", number == 0)
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
    for rid in requested:
        needs = load_recipe(args.recipes_dir, rid).get("requires")
        if needs and needs not in requested:
            raise UsageError("%s needs the %s recipe" % (rid, needs))
    if session:
        units.append({"key": "desktop-conf", "title": "Desktop session (%s)" % session, "session": session,
                      "pre": [], "apt": [], "debs": [], "post": []})
    return requested, units, chosen_all


def run_plan(args, requested, units, chosen_all):
    total = len(units)
    runner = Runner(True, args.assets_dir, args.log_file)
    if any(u.get("apt") or u.get("debs") or u.get("pre") for u in units):
        runner.quiet_guest()
        runner.write_apt_conf()
        runner.pick_mirror()
        print("  + apt-get update", flush=True)
    for number, unit in enumerate(units, 1):
        progress(number, total, "Would install " + unit["title"])
        for line in unit.get("pre", []):
            runner.shell(line, unit["title"])
        if unit.get("pre"):
            print("  + apt-get update", flush=True)
        # one apt run for the packages and the downloaded .deb files together
        deb_paths = [runner.fetch_deb(deb) for deb in unit.get("debs", [])]
        if unit.get("apt") or deb_paths:
            runner.apt_install(list(unit.get("apt", [])) + deb_paths, unit["title"])
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
    started = time.monotonic()
    adig = assets_digest(args.assets_dir)
    state = load_state(args.state_file)
    pending = [u for u in units if state.get(u["key"]) != unit_digest(u, adig)]
    runner = Runner(False, args.assets_dir, args.log_file)
    runner._append("== apply %s" % " ".join(requested))
    needs_apt = any(u["apt"] or u["debs"] or u["pre"] for u in pending)
    weights = [unit_weight(u) for u in pending]
    prep = 0.04 * sum(weights) if needs_apt else 0.0
    reporter = Reporter(([prep] if needs_apt else []) + (weights or [1.0]))
    offset = 1 if needs_apt else 0
    reporter.update(0.0, "Getting ready", True)
    if needs_apt:
        # Nothing else may hold apt, then the nearest fast server, then the first list refresh.
        runner.quiet_guest()
        runner.write_apt_conf()
        reporter.update(0.05, "Choosing the fastest download server", True)
        server = runner.pick_mirror()
        # Chrome and the other recipe .deb files only need the network: start them now, beside the first list refresh
        # (not before the mirror pick, whose speed test a running download would distort).
        runner.start_early_debs(pending)
        reporter.update(0.5, "Downloading from %s" % server, True)
        runner.apt_update(lambda f, t: reporter.update(0.5 + 0.5 * f, t))
    installed = []
    skipped = []
    todo = {u["key"]: i for i, u in enumerate(pending)}
    for unit in units:
        if unit["key"] not in todo:
            skipped.append(unit["key"])
            continue
        reporter.begin(offset + todo[unit["key"]])
        up = UnitProgress(reporter, unit["title"], unit_stages(unit))
        reporter.update(0.0, "%s: starting" % unit["title"], True)
        run_unit(runner, unit, up, [u for u in units if u["key"] in todo and todo[u["key"]] > todo[unit["key"]]])
        state[unit["key"]] = unit_digest(unit, adig)
        save_state(args.state_file, state)
        installed.append(unit["key"])
        reporter.update(1.0, "%s: done" % unit["title"], True)
    reporter.finish("Everything is installed")
    emit_result({"status": "ok", "dry_run": False, "recipes": requested, "options": chosen_all,
                 "installed": installed, "skipped": skipped, "mirror": runner.mirror,
                 "downloaded_mb": round(runner.downloaded / 1048576.0, 1),
                 "seconds": round(time.monotonic() - started, 1)})
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

"""Contract checks for apply-recipe.py's speed work: the mirror pick, the parallel downloads, the progress numbers.

No distro, no network, no root: the pieces are pure functions or run against fakes. The real thing is timed in the sandbox
(see the speed notes in recipes/SCHEMA.md).

Run: python -I -m unittest discover -s . -p "test_*.py"
"""

import contextlib
import importlib.machinery
import importlib.util
import io
import json
import os
import subprocess
import sys
import threading
import time
import unittest
from datetime import datetime, timedelta, timezone

HERE = os.path.dirname(os.path.abspath(__file__))
RECIPES = os.path.join(HERE, "..", "..", "recipes")


def _load():
    loader = importlib.machinery.SourceFileLoader("apply_recipe", os.path.join(HERE, "apply-recipe.py"))
    spec = importlib.util.spec_from_loader("apply_recipe", loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


ar = _load()


def recipe(name):
    with open(os.path.join(RECIPES, name + ".json"), encoding="utf-8") as handle:
        return json.load(handle)


def captured(fn, *args, **kwargs):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        result = fn(*args, **kwargs)
    return result, out.getvalue()


class PrintUrisTests(unittest.TestCase):
    TEXT = (
        "'http://free.nchc.org.tw/ubuntu/pool/main/f/foo/foo_1.0-1_amd64.deb' foo_1.0-1_amd64.deb 12345 SHA256:" + "a" * 64 + "\n"
        "'http://x/ubuntu/pool/main/b/bar/bar_2%3a1.0_all.deb' bar_2%3a1.0_all.deb 99 MD5Sum:abcd\n"
        "WARNING: something\n"
        "'ftp://x/y.deb' y.deb 1 SHA256:" + "b" * 64 + "\n"
        "'http://x/evil' ../evil.deb 1 SHA256:" + "c" * 64 + "\n"
    )

    def test_items_are_read_and_bad_lines_dropped(self):
        items = ar.parse_print_uris(self.TEXT)
        self.assertEqual([i["name"] for i in items], ["foo_1.0-1_amd64.deb", "bar_2%3a1.0_all.deb"])
        self.assertEqual(items[0]["size"], 12345)
        self.assertEqual(items[0]["sha256"], "a" * 64)
        self.assertIsNone(items[1]["sha256"], "only SHA256 is checked here; apt checks the rest itself")

    def test_the_install_count_is_the_inst_lines(self):
        text = "Reading package lists...\nInst libfoo (1.0 Ubuntu:24.04/noble [amd64])\nInst bar (2 x)\nConf libfoo (1.0)\n"
        self.assertEqual(ar.count_installs(text), 2)


class MirrorTests(unittest.TestCase):
    def test_the_list_keeps_only_http_archives_for_amd64(self):
        text = "\n".join([
            "http://free.nchc.org.tw/ubuntu/", "https://mirror.twds.com.tw/ubuntu-ports/", "ftp://old.example/ubuntu/",
            "https://mirror.twds.com.tw/ubuntu", "garbage", "http://free.nchc.org.tw/ubuntu/", ""])
        self.assertEqual(ar.parse_mirror_list(text), ["http://free.nchc.org.tw/ubuntu/", "https://mirror.twds.com.tw/ubuntu/"])

    def test_the_release_date_is_read(self):
        text = "-----BEGIN PGP SIGNED MESSAGE-----\nOrigin: Ubuntu\nSuite: noble-updates\nDate: Sat, 01 Nov 2026 12:30:00 UTC\n"
        self.assertEqual(ar.release_date(text), datetime(2026, 11, 1, 12, 30, tzinfo=timezone.utc))
        self.assertIsNone(ar.release_date("Origin: Ubuntu\n"))

    def test_a_stale_mirror_is_dropped_and_the_fastest_sample_wins(self):
        now = datetime(2026, 11, 1, 12, 0, tzinfo=timezone.utc)
        stage1 = [
            {"base": "A", "latency": 0.05, "date": now - timedelta(days=3)},   # fastest, but three days behind
            {"base": "B", "latency": 0.30, "date": now - timedelta(hours=2)},
            {"base": "C", "latency": 0.10, "date": now - timedelta(hours=1)},
            {"base": "D", "latency": 1.50, "date": now},
        ]
        best, why = ar.choose_best(stage1, {"B": 0.15, "C": 0.40})
        self.assertEqual(best["base"], "B", why)
        best, _ = ar.choose_best(stage1, {})
        self.assertEqual(best["base"], "C", "without samples the first answer decides")
        self.assertIsNone(ar.choose_best([], {})[0])
        self.assertIsNone(ar.choose_best([{"base": "X", "latency": 1, "date": None}], {})[0])

    def test_the_sources_file_is_pointed_at_the_mirror_and_nothing_else_changes(self):
        text = (
            "## header comment\nTypes: deb\nURIs: http://archive.ubuntu.com/ubuntu/\nSuites: noble noble-updates noble-backports\n"
            "Components: main universe restricted multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\n"
            "## security\nTypes: deb\nURIs: http://security.ubuntu.com/ubuntu/\nSuites: noble-security\n"
            "Components: main universe restricted multiverse\nSigned-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg\n\n"
            "Types: deb\nURIs: https://example.com/other\nSuites: stable\nComponents: main\n")
        new = ar.rewrite_sources(text, "noble", "http://free.nchc.org.tw/ubuntu/", "http://security.ubuntu.com/ubuntu/")
        self.assertIn("URIs: http://free.nchc.org.tw/ubuntu/\nSuites: noble noble-updates noble-backports", new)
        self.assertIn("URIs: http://security.ubuntu.com/ubuntu/\nSuites: noble-security", new)
        self.assertIn("URIs: https://example.com/other", new, "a stanza that is not Ubuntu's is left alone")
        self.assertIn("## header comment", new)
        self.assertIn("Signed-By: /usr/share/keyrings/ubuntu-archive-keyring.gpg", new)
        same_mirror = ar.rewrite_sources(text, "noble", "http://m/ubuntu/", "http://m/ubuntu/")
        self.assertEqual(same_mirror.count("URIs: http://m/ubuntu/"), 2)
        self.assertNotIn("archive.ubuntu.com", same_mirror)

    def test_the_parallel_runner_gives_up_on_a_slow_job(self):
        def fast():
            return "fast"

        def slow():
            time.sleep(3)
            return "slow"

        started = time.monotonic()
        results = ar.run_parallel([fast, slow], 0.4)
        self.assertEqual(results, ["fast"])
        self.assertLess(time.monotonic() - started, 1.5)

    def test_a_job_that_raises_is_not_a_candidate(self):
        def boom():
            raise OSError("down")

        self.assertEqual(ar.run_parallel([boom, lambda: 1], 1.0), [1])


class LookAheadTests(unittest.TestCase):
    def unit(self, key, apt=("a",), pre=(), early=False, session=None, debs=()):
        u = {"key": key, "title": key, "pre": list(pre), "apt": list(apt), "debs": list(debs), "post": []}
        if early:
            u["early_pre"] = True
        if session:
            u["session"] = session
        return u

    def test_a_unit_without_pre_lines_may_start_early(self):
        nxt = self.unit("xfce")
        self.assertIs(ar.lookahead_candidate([nxt, self.unit("coding", pre=["x"])]), nxt)

    def test_post_only_and_session_units_are_passed_over(self):
        post_only = self.unit("pnpm", apt=())
        nxt = self.unit("coding", pre=["curl ..."], early=True)
        self.assertIs(ar.lookahead_candidate([post_only, self.unit("s", apt=(), session="xfce4"), nxt]), nxt)

    def test_pre_lines_without_the_flag_stop_the_chain(self):
        self.assertIsNone(ar.lookahead_candidate([self.unit("zorin", pre=["add-apt-repository"]), self.unit("later")]))
        self.assertIsNone(ar.lookahead_candidate([]))

    def test_the_coding_recipe_says_its_pre_lines_may_run_early(self):
        coding = recipe("coding")
        self.assertIs(coding.get("early_pre"), True)
        text = " ".join(coding["pre"])
        for tool in ("install", "curl", "gpg", "echo"):
            self.assertIn(tool, text)  # only tools a stock Ubuntu has
        self.assertNotIn("apt-get", text)

    def test_the_flag_comes_through_the_unit_and_does_not_change_its_digest(self):
        unit = ar.make_unit("coding", "Coding tools", recipe("coding"))
        self.assertTrue(unit["early_pre"])
        plain = {k: v for k, v in unit.items() if k != "early_pre"}
        self.assertEqual(ar.unit_digest(unit, "h"), ar.unit_digest(plain, "h"))
        with self.assertRaises(ar.UsageError):
            ar.check_block({"early_pre": "yes"}, "x")


class RunnerTests(unittest.TestCase):
    def setUp(self):
        import tempfile
        self.dir = tempfile.TemporaryDirectory()
        self.addCleanup(self.dir.cleanup)
        self.log = os.path.join(self.dir.name, "log", "apply.log")
        self.runner = ar.Runner(False, "/x/assets", self.log)

    def test_stream_hands_over_each_line_logs_it_and_reports_failure(self):
        seen = []
        self.runner.stream([sys.executable, "-c", "print('Unpacking a');print('Setting up a')"], "t", "test", on_line=seen.append)
        self.assertEqual(seen, ["Unpacking a", "Setting up a"])
        with open(self.log, encoding="utf-8") as handle:
            self.assertIn("Setting up a", handle.read())
        with self.assertRaises(ar.StepError):
            self.runner.stream([sys.executable, "-c", "import sys;sys.exit(100)"], "t", "test")

    def test_stream_ticks_while_a_command_is_quiet(self):
        ticks = []
        self.runner.stream([sys.executable, "-c", "import time;time.sleep(0.35)"], "t", "test", on_tick=lambda: ticks.append(1), tick=0.1)
        self.assertGreaterEqual(len(ticks), 2)

    def test_a_failed_mirror_puts_the_original_sources_back(self):
        sources = os.path.join(self.dir.name, "ubuntu.sources")
        orig = os.path.join(self.dir.name, "orig")
        with open(sources, "w", encoding="utf-8") as handle:
            handle.write("URIs: http://mirror.example/ubuntu/" + chr(10))
        with open(orig, "w", encoding="utf-8") as handle:
            handle.write("URIs: http://archive.ubuntu.com/ubuntu/" + chr(10))
        old = (ar.SOURCES_FILE, ar.SOURCES_ORIG)
        ar.SOURCES_FILE, ar.SOURCES_ORIG = sources, orig
        self.addCleanup(lambda: setattr(ar, "SOURCES_FILE", old[0]) or setattr(ar, "SOURCES_ORIG", old[1]))
        self.assertFalse(self.runner.fall_back_to_archive(), "no mirror was picked: nothing to undo")
        self.runner.mirror = "http://mirror.example/ubuntu/"
        self.assertTrue(self.runner.fall_back_to_archive())
        with open(sources, encoding="utf-8") as handle:
            self.assertIn("archive.ubuntu.com", handle.read())
        self.assertIsNone(self.runner.mirror)
        self.assertFalse(self.runner.fall_back_to_archive(), "only once")


class ReporterTests(unittest.TestCase):
    def lines(self, out):
        rows = []
        for line in out.splitlines():
            parts = line.split(" ", 2)
            n, total = parts[1].split("/")
            rows.append((int(n), int(total), parts[2] if len(parts) > 2 else ""))
        return rows

    def test_the_number_never_goes_down_and_is_1000_only_at_the_end(self):
        clock = [0.0]
        rep = ar.Reporter([10.0, 30.0, 60.0], clock=lambda: clock[0])

        def run():
            rep.update(0.0, "start", True)
            for index, fractions in enumerate([(0.2, 0.9, 1.0), (0.5, 0.4, 1.0), (0.1, 1.0)]):
                rep.begin(index)
                for fraction in fractions:
                    clock[0] += 1.0
                    rep.update(fraction, "x%d %s" % (index, fraction))
            rep.finish("done")

        _, out = captured(run)
        rows = self.lines(out)
        numbers = [r[0] for r in rows]
        self.assertEqual(numbers, sorted(numbers))
        self.assertTrue(all(r[1] == 1000 for r in rows))
        self.assertEqual(rows[-1][:2], (1000, 1000))
        self.assertTrue(all(n < 1000 for n in numbers[:-1]))

    def test_lines_are_not_repeated_or_flooded(self):
        clock = [0.0]
        rep = ar.Reporter([1.0], clock=lambda: clock[0])
        _, out = captured(lambda: [rep.update(0.5, "same"), rep.update(0.5, "same"), rep.update(0.51, "other"),
                                   rep.update(0.52, "forced", True)])
        texts = [r[2] for r in self.lines(out)]
        self.assertEqual(texts, ["same", "forced"], "the same line twice and a line inside the gap are dropped")

    def test_units_weigh_what_they_install_and_a_recipe_can_say_so(self):
        small = {"apt": ["a"], "debs": [], "post": [], "pre": []}
        big = {"apt": ["a"] * 20, "debs": [{"url": "https://x/y.deb"}], "post": ["x"] * 10, "pre": []}
        self.assertLess(ar.unit_weight(small), ar.unit_weight(big))
        self.assertEqual(ar.unit_weight(dict(small, size_mb=123)), 123.0)

    def test_stages_follow_what_the_unit_has(self):
        names = lambda u: [n for n, _ in ar.unit_stages(u)]
        self.assertEqual(names({"pre": [], "apt": ["a"], "debs": [], "post": []}), ["download", "install"])
        self.assertEqual(names({"pre": ["l"], "apt": ["a"], "debs": [], "post": ["p"]}), ["pre", "download", "install", "post"])
        self.assertEqual(names({"pre": [], "apt": [], "debs": [], "post": ["p"]}), ["post"])

    def test_the_unit_text_names_the_unit(self):
        rep = ar.Reporter([1.0])
        up = ar.UnitProgress(rep, "XFCE desktop", ar.unit_stages({"pre": [], "apt": ["a"], "debs": [], "post": []}))
        _, out = captured(lambda: up.at("download", 0.5, "Downloading 1 of 2 MB", True))
        self.assertIn("XFCE desktop: Downloading 1 of 2 MB", out)
        self.assertTrue(out.startswith("PROGRESS "))
        n = int(out.split()[1].split("/")[0])
        self.assertTrue(0 < n < 1000)


class DownloadTests(unittest.TestCase):
    def test_the_line_has_sizes_speed_and_time_left(self):
        mb = 1048576
        text = ar.download_line(120 * mb, 340 * mb, 4.1 * mb)
        self.assertTrue(text.startswith("Downloading 120 of 340 MB"), text)
        self.assertIn("4.1 MB/s", text)
        self.assertIn("about 53 s left", text)
        self.assertEqual(ar.download_line(0, 0, 0), "Downloading")
        self.assertNotIn("left", ar.download_line(1 * mb, 340 * mb, 0))

    def test_the_fetcher_runs_tasks_side_by_side_and_counts_bytes(self):
        started = []
        gate = threading.Barrier(3, timeout=5)

        def make(size):
            def run(on_bytes):
                started.append(size)
                gate.wait()  # three tasks must be running at the same moment
                on_bytes(size)
            return ar.FetchTask("t%d" % size, size, run, required=False)

        fetcher = ar.Fetcher([make(10), make(30), make(20)], workers=3)
        self.assertEqual(fetcher.total, 60)
        fetcher.start()
        seen = []
        ar.wait_fetchers([fetcher], lambda done, total, speed: seen.append((done, total)), interval=0.05)
        self.assertEqual(fetcher.done, 60)
        self.assertEqual(seen[-1], (60, 60))
        self.assertEqual(sorted(started), [10, 20, 30])

    def test_biggest_first_and_a_failed_task_is_recorded_not_raised(self):
        order = []
        lock = threading.Lock()

        def make(name, size, fail=False):
            def run(on_bytes):
                with lock:
                    order.append(name)
                if fail:
                    raise ar.StepError("no route")
                on_bytes(size)
            return ar.FetchTask(name, size, run, required=False)

        tasks = [make("small", 1), make("big", 100, fail=True), make("mid", 10)]
        fetcher = ar.Fetcher(tasks, workers=1)
        fetcher.start()
        ar.wait_fetchers([fetcher], lambda *a: None, interval=0.02)
        self.assertEqual(order, ["big", "mid", "small"])
        self.assertEqual([t.error for t in tasks], [None, "no route", None])

    def test_a_retry_takes_its_bytes_back(self):
        # on_bytes(-n) after a failed attempt keeps the bar honest
        fetcher = ar.Fetcher([], workers=1)
        fetcher._on_bytes(50)
        fetcher._on_bytes(-50)
        self.assertEqual(fetcher.done, 0)


class PlanTests(unittest.TestCase):
    """The dry run prints the commands without root, a distro or a network."""

    def run_plan(self, *extra):
        script = os.path.join(HERE, "apply-recipe.py")
        proc = subprocess.run([sys.executable, "-I", script, "--recipes-dir", RECIPES, "--dry-run", "--recipe", "xfce",
                               "--recipe", "coding", "--opt", "coding.pkg=pnpm"] + list(extra),
                              capture_output=True, text=True, timeout=60)
        self.assertEqual(proc.returncode, 0, proc.stderr)
        return proc.stdout

    def test_every_apt_install_leaves_out_recommends_and_quiets_the_update_jobs(self):
        out = self.run_plan()
        installs = [l for l in out.splitlines() if "apt-get install" in l]
        self.assertTrue(installs)
        for line in installs:
            self.assertIn("--no-install-recommends", line)
        self.assertIn("mask packagekit, unattended-upgrades", out)
        self.assertIn("choose the fastest nearby Ubuntu mirror", out)
        self.assertLess(out.index("choose the fastest"), out.index("apt-get update"))
        self.assertLess(out.index("mask packagekit"), out.index("apt-get update"))

    def test_chrome_and_the_base_packages_install_in_one_apt_run(self):
        out = self.run_plan()
        base = [l for l in out.splitlines() if "apt-get install" in l and "xvfb" in l]
        self.assertEqual(len(base), 1)
        self.assertIn("google-chrome-stable_current_amd64.deb", base[0])

    def test_novnc_is_not_an_apt_dependency_of_the_base(self):
        base = recipe("base")
        self.assertNotIn("novnc", base["apt"])
        out = self.run_plan()
        self.assertIn("pm-sbx-novnc-lite.sh", out)
        self.assertNotRegex(out, r"apt-get install[^\n]* novnc( |$)")

    def test_the_desktop_things_the_spec_needs_are_still_in_the_base(self):
        base = recipe("base")
        for needed in ("xvfb", "x11vnc", "websockify", "xdotool", "imagemagick", "xclip", "fonts-noto-cjk",
                       "fonts-noto-color-emoji", "xterm", "pcmanfm", "mousepad", "evince"):
            self.assertIn(needed, base["apt"])
        xfce = recipe("xfce")
        self.assertIn("xfce4-session", xfce["apt"])

    def test_a_bad_size_estimate_is_refused(self):
        with self.assertRaises(ar.UsageError):
            ar.check_block({"size_mb": -1}, "x")
        with self.assertRaises(ar.UsageError):
            ar.check_block({"size_mb": "big"}, "x")
        ar.check_block({"size_mb": 12.5}, "x")

    def test_a_changed_estimate_does_not_make_a_finished_step_run_again(self):
        unit = {"key": "k", "title": "t", "pre": [], "apt": ["a"], "debs": [], "post": []}
        self.assertEqual(ar.unit_digest(unit, "h"), ar.unit_digest(dict(unit, size_mb=99), "h"))


if __name__ == "__main__":
    unittest.main()

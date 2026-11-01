"""Contract checks for the desktop scripts: who owns the secrets, the one boot verdict, the one place the control service starts.

These run without a distro. The shell functions that handle the secrets are cut out of pm-sbx-start-desktop.sh and run
in bash with a fake /run/pm-sbx folder; the rest is pinned by reading the scripts. The real thing (WSL, Xvfb, x11vnc, Chrome)
is covered by the opt-in live test in the app (PM_SBX_LIVE=1, potato::sandbox::live).

Run: python -I -m unittest discover -s . -p "test_*.py"
"""

import os
import re
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
START = os.path.join(HERE, "pm-sbx-start-desktop.sh")
BOOT = os.path.join(HERE, "pm-sbx-boot.sh")
NOWSLG = os.path.join(HERE, "pm-no-wslg.sh")
BASH = shutil.which("bash")


def read(path):
    with open(path, encoding="utf-8", newline="") as handle:
        return handle.read()


def function_text(script, name):
    """The text of `name() { ... }`: the one line when it is written on one line, else up to the line that is exactly `}`."""
    nl = chr(10)
    start = script.find(nl + name + "() {")
    if start < 0:
        raise AssertionError("function %s not found" % name)
    start += 1
    first_end = script.index(nl, start)
    first = script[start:first_end]
    if first.rstrip().endswith("}"):
        return first
    end = script.index(nl + "}" + nl, start) + 2
    return script[start:end]


class ShellFixture:
    """Runs the secret-handling functions of the start script against a temp folder standing in for /run/pm-sbx."""

    FUNCTIONS = ["rand8", "valid_vnc_password", "make_vnc_passwords"]

    def __init__(self, test):
        self.dir = tempfile.mkdtemp(prefix="pm-sbx-test-")
        test.addCleanup(shutil.rmtree, self.dir, True)
        self.run_dir = os.path.join(self.dir, "run")
        self.vnc_dir = os.path.join(self.dir, "state", "vnc")
        os.makedirs(self.run_dir)
        os.makedirs(self.vnc_dir)
        self.script = read(START)
        self.preamble = "\n".join(function_text(self.script, name) for name in self.FUNCTIONS)

    def push(self, name, value):
        with open(os.path.join(self.run_dir, name), "w", encoding="utf-8", newline="") as handle:
            handle.write(value)

    def run(self, body):
        """Runs `make_vnc_passwords` and then `body`; returns (exit code, stdout)."""
        run_dir = self.run_dir.replace("\\", "/")
        vnc_dir = self.vnc_dir.replace("\\", "/")
        code = (
            "set -u\n"
            'PUSHED_VIEW="%s/vnc-view.pass"\nPUSHED_CONTROL="%s/vnc-control.pass"\nVNCDIR="%s"\n'
            'log() { echo "LOG: $*"; }\n'
            'die() { echo "DIE($2): $1"; exit "${2:-1}"; }\n'
            "%s\n"
            "make_vnc_passwords\n"
            "%s\n"
        ) % (run_dir, run_dir, vnc_dir, self.preamble, body)
        result = subprocess.run([BASH, "-c", code], capture_output=True, text=True, timeout=60)
        return result.returncode, result.stdout.replace("\r", "")

    def file(self, name):
        with open(os.path.join(self.vnc_dir, name), encoding="utf-8", newline="") as handle:
            return handle.read()


@unittest.skipUnless(BASH, "bash is not installed")
class AppOwnedVncPasswords(unittest.TestCase):
    def test_pushed_passwords_are_used_not_invented(self):
        fx = ShellFixture(self)
        fx.push("vnc-view.pass", "Vi3wPass")
        fx.push("vnc-control.pass", "Ctr1Pass")
        code, out = fx.run('echo "SOURCE=$VNC_SOURCE CHANGED=$VNC_CHANGED"')
        self.assertEqual(code, 0, out)
        self.assertIn("SOURCE=from the app CHANGED=1", out)
        self.assertNotIn("FALLBACK", out)
        self.assertEqual(fx.file("view.plain"), "Vi3wPass")
        self.assertEqual(fx.file("control.plain"), "Ctr1Pass")
        # x11vnc's file: control section first, then the view-only section
        self.assertEqual(fx.file("passwd"), "Ctr1Pass\n__BEGIN_VIEWONLY__\nVi3wPass\n")

    def test_the_same_push_changes_nothing_and_a_new_push_is_noticed(self):
        fx = ShellFixture(self)
        fx.push("vnc-view.pass", "Vi3wPass")
        fx.push("vnc-control.pass", "Ctr1Pass")
        fx.run("true")
        code, out = fx.run('echo "CHANGED=$VNC_CHANGED"')
        self.assertIn("CHANGED=0", out, "x11vnc is not restarted for the same passwords")
        fx.push("vnc-view.pass", "N3wVi3wP")
        code, out = fx.run('echo "CHANGED=$VNC_CHANGED"')
        self.assertIn("CHANGED=1", out, "a running x11vnc still holds the old ones")
        self.assertEqual(fx.file("view.plain"), "N3wVi3wP")

    def test_a_pushed_file_that_is_unusable_stops_the_start_it_never_falls_back(self):
        cases = {
            "too short": ("Short1", "Ctr1Pass"),
            "equal": ("SamePass1", "SamePass1"),
            "not alphanumeric": ("Vi3w Pas", "Ctr1Pass"),
            "empty": ("", "Ctr1Pass"),
        }
        for label, (view, control) in cases.items():
            with self.subTest(label):
                fx = ShellFixture(self)
                fx.push("vnc-view.pass", view)
                fx.push("vnc-control.pass", control)
                code, out = fx.run("echo SHOULD_NOT_GET_HERE")
                self.assertEqual(code, 3, out)
                self.assertIn("DIE(3)", out)
                self.assertNotIn("SHOULD_NOT_GET_HERE", out)
                self.assertNotIn("FALLBACK", out, "the guest must not invent passwords when the app gave bad ones")
                self.assertFalse(os.path.exists(os.path.join(fx.vnc_dir, "passwd")))

    def test_only_one_of_the_two_pushed_files_is_refused(self):
        fx = ShellFixture(self)
        fx.push("vnc-view.pass", "Vi3wPass")
        code, out = fx.run("echo SHOULD_NOT_GET_HERE")
        self.assertEqual(code, 3, out)
        self.assertNotIn("FALLBACK", out)

    def test_without_a_push_the_fallback_runs_and_says_so(self):
        fx = ShellFixture(self)
        code, out = fx.run('echo "SOURCE=$VNC_SOURCE"')
        self.assertEqual(code, 0, out)
        self.assertIn("LOG: FALLBACK: generating VNC passwords here", out)
        self.assertIn("SOURCE=self-generated fallback", out)
        view, control = fx.file("view.plain"), fx.file("control.plain")
        self.assertRegex(view, r"^[A-Za-z0-9]{8}$")
        self.assertRegex(control, r"^[A-Za-z0-9]{8}$")
        self.assertNotEqual(view, control)
        # a second manual start reuses them, and still says it is the fallback
        code, out = fx.run("true")
        self.assertIn("FALLBACK: reusing", out)
        self.assertEqual(fx.file("view.plain"), view)


class ScriptContract(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.start = read(START)
        cls.boot = read(BOOT)
        cls.nowslg = read(NOWSLG)

    def test_scripts_are_lf_and_parse(self):
        for path in (START, BOOT, NOWSLG):
            self.assertNotIn("\r", read(path), path)
            if BASH:
                result = subprocess.run([BASH, "-n", path], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, path + result.stderr)

    def test_the_pushed_files_are_the_ones_the_app_writes(self):
        # the app (secrets.rs) pushes exactly these three paths
        self.assertIn("PUSHED_TOKEN=/run/pm-sbx/control.token", self.start)
        self.assertIn("PUSHED_VIEW=/run/pm-sbx/vnc-view.pass", self.start)
        self.assertIn("PUSHED_CONTROL=/run/pm-sbx/vnc-control.pass", self.start)

    def test_the_control_service_is_started_here_with_the_pushed_token(self):
        body = function_text(self.start, "start_control")
        self.assertIn('TOKEN_FILE="$PUSHED_TOKEN"', body)
        self.assertIn('PM_SBX_CONTROL_TOKEN_FILE="$TOKEN_FILE"', body)
        # the self-generated token exists only in the branch that logs FALLBACK
        generate = body.index("head -c 512 /dev/urandom")
        self.assertLess(body.rindex("FALLBACK", 0, generate), generate)
        self.assertIn("is empty or not readable", body, "a bad pushed token stops the start, it does not fall back")
        self.assertEqual(self.start.count("\n  start_control\n"), 1, "do_start calls it once")

    def test_stop_stops_chrome_first(self):
        body = function_text(self.start, "do_stop")
        self.assertLess(body.index("stop_chrome"), body.index("stop_one control"))
        chrome = function_text(self.start, "chrome_pids")
        self.assertIn("/opt/google/chrome/chrome", chrome)
        self.assertIn('-u "$(id -u)"', chrome, "only this user's Chrome")
        self.assertIn("kill -9", function_text(self.start, "stop_chrome"), "a Chrome that ignores TERM is killed")

    def test_the_desktop_still_refuses_without_the_boot_verdict(self):
        gate = function_text(self.start, "gate")
        self.assertIn("BOOT_DONE=/run/pm-sbx/boot.done", self.start)
        self.assertIn("BOOT_FAILED=/run/pm-sbx/boot.failed", self.start)
        for needle in ("$BOOT_DONE", "$BOOT_FAILED", "/mnt/wslg", "state=passed"):
            self.assertIn(needle, gate)

    def test_the_boot_hook_is_the_only_writer_of_the_verdict(self):
        self.assertIn('"$STATUS_DIR/boot.done"', self.boot)
        self.assertIn('"$STATUS_DIR/boot.failed"', self.boot)
        # the WSLg cut writes no flag of its own any more
        self.assertNotRegex(self.nowslg, r"touch\s+/run/pm-no-wslg")
        for text in (self.start, self.boot, self.nowslg):
            code = "".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
            self.assertNotIn("pm-no-wslg.done", code)
            self.assertNotIn("pm-no-wslg.failed", code)

    def test_no_secret_is_logged_or_put_on_a_command_line(self):
        # values are read from files; the log lines name the source only
        for line in self.start.splitlines():
            if line.lstrip().startswith("log ") or "log \"" in line:
                self.assertNotIn("$ctl", line)
                self.assertNotIn("$view", line)
                self.assertNotIn("$(cat", line)


if __name__ == "__main__":
    unittest.main()

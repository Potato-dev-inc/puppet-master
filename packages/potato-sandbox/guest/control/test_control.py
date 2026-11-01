#!/usr/bin/env python3
# Ported from Rakazo (https://github.com/elie222/rakazo), commit f48fa5c7, file infra/sandboxes/computer/control.py.
# Copyright 2026 Rakazo contributors. Licensed under the Apache License, Version 2.0;
# see packages/potato-sandbox/LICENSE-APACHE-2.0. Changed by Puppet Master: new unit tests written for the ported
# pm-sbx-control and pm-sbx-focus-or-launch (allowlist, argv building, config, auth, wrapper matching). No X server,
# no real subprocess: subprocess and capture are mocked. Not a copy of any upstream test file.
"""Run with: python -I -m unittest discover -s <this folder> -p "test_*.py"
"""

import hmac
import importlib.machinery
import io
import importlib.util
import json
import os
import subprocess
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from unittest import mock

HERE = os.path.dirname(os.path.abspath(__file__))


def _load(module_name, filename):
    path = os.path.join(HERE, filename)
    loader = importlib.machinery.SourceFileLoader(module_name, path)
    spec = importlib.util.spec_from_loader(module_name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


control = _load("pm_sbx_control", "pm-sbx-control")
focus = _load("pm_sbx_focus_or_launch", "pm-sbx-focus-or-launch")

DISPLAY = ":99"
TOKEN = "t0k3n-for-tests-0123456789"
PROFILE = "/home/pm/.browser-profiles/chromium-bot-" + "a" * 32
FOCUS = "pm-sbx-focus-or-launch"
LAUNCH = ["env", f"DISPLAY={DISPLAY}", FOCUS]
PROFILE_ARG = f"PM_SBX_BROWSER_PROFILE={PROFILE}"


def settings_for(launchers=None, token=TOKEN, wrapper="/opt/pm-sbx/pm-sbx-focus-or-launch"):
    return control.Settings(
        token=token,
        port=0,
        default_display=DISPLAY,
        launchers=dict(control.DEFAULT_LAUNCHERS) if launchers is None else launchers,
        focus_wrapper=wrapper,
    )


def xdotool(*args):
    return ["env", f"DISPLAY={DISPLAY}", "xdotool", *args]


class XdotoolAllowlistTests(unittest.TestCase):
    def allowed(self, argv):
        return control.allowed_control_argv(argv, DISPLAY, control.DEFAULT_LAUNCHERS)

    def test_emitted_forms_are_allowed(self):
        cases = [
            xdotool("key", "--clearmodifiers", "ctrl+c"),
            xdotool("mousemove", "--", "10", "20"),
            xdotool("mousemove", "--", "-5", "0"),
            xdotool("mousemove", "--", "10", "20", "mousedown", "1"),
            xdotool("mousemove", "--", "10", "20", "click", "3"),
            xdotool("mousemove", "--", "10", "20", "click", "--repeat", "3", "5"),
            xdotool("mouseup", "1"),
            xdotool("mouseup", "3"),
            xdotool("type", "--clearmodifiers", "--", "hello world"),
            xdotool("click", "--repeat", "2", "4"),
            xdotool("click", "--repeat", "20", "7"),
        ]
        for argv in cases:
            with self.subTest(argv=argv):
                self.assertTrue(self.allowed(argv))

    def test_rejected_xdotool_forms(self):
        cases = [
            xdotool("key", "ctrl+c"),  # missing --clearmodifiers
            xdotool("key", "--clearmodifiers", ""),  # empty key
            xdotool("key", "--clearmodifiers", "ctrl", "extra"),
            xdotool("mousemove", "--", "1;ls", "2"),  # not an integer
            xdotool("mousemove", "--", "1", "2", "mousedown", "2"),  # button 2
            xdotool("mousemove", "--", "1", "2", "click", "9"),
            xdotool("mouseup", "2"),
            xdotool("type", "--clearmodifiers", "hello"),  # missing --
            xdotool("click", "--repeat", "0", "4"),  # repeat below range
            xdotool("click", "--repeat", "21", "4"),  # repeat above range
            xdotool("click", "--repeat", "+3", "4"),  # not plain digits
            xdotool("click", "--repeat", "2", "1"),  # left button is not a wheel
            xdotool("click", "1"),  # no --repeat
            xdotool("windowactivate", "1"),  # op not in the allowlist
            xdotool("getwindowname", "1"),
        ]
        for argv in cases:
            with self.subTest(argv=argv):
                self.assertFalse(self.allowed(argv))

    def test_integers_are_ascii_digits_only(self):
        # str.isdigit() is true for "²" (superscript two), but int() rejects it and xdotool would too.
        for value in ("²", "-²", "1²", "٣", "１"):  # also Arabic-Indic and fullwidth digits
            with self.subTest(value=value):
                self.assertFalse(control._is_int_string(value))
                self.assertFalse(self.allowed(xdotool("mousemove", "--", value, "5")))
                self.assertFalse(self.allowed(xdotool("mousemove", "--", "5", value)))
                self.assertFalse(self.allowed(xdotool("mousemove", "--", "5", "5", "click", "--repeat", value, "5")))
        for value in ("0", "7", "-5", "1234"):
            with self.subTest(value=value):
                self.assertTrue(control._is_int_string(value))
        for value in ("", "-", "+1", "1.5", " 1", "1\n"):
            with self.subTest(value=value):
                self.assertFalse(control._is_int_string(value))

    def test_xdotool_shape_is_checked_on_argv_not_on_strings(self):
        self.assertFalse(self.allowed("env DISPLAY=:99 xdotool mouseup 1"))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}"]))
        self.assertFalse(self.allowed(["sh", f"DISPLAY={DISPLAY}", "xdotool", "mouseup", "1"]))
        self.assertFalse(self.allowed(["env", "DISPLAY=:1", "xdotool", "mouseup", "1"]))
        self.assertFalse(self.allowed(["env", DISPLAY, "xdotool", "mouseup", "1"]))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", "xdotool", "type", "--clearmodifiers", "--", "a\0b"]))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", "xdotool", "mouseup", 1]))
        self.assertFalse(self.allowed(xdotool("type", "--clearmodifiers", "--", "x" * (control.MAX_ARG_LEN + 1))))
        too_many = ["env", f"DISPLAY={DISPLAY}", "xdotool", "type", "--clearmodifiers", "--"]
        too_many += ["a"] * (control.MAX_ARGV - len(too_many) + 1)
        self.assertFalse(self.allowed(too_many))
        self.assertFalse(self.allowed(None))
        self.assertFalse(self.allowed(tuple(xdotool("mouseup", "1"))))


class LaunchAllowlistTests(unittest.TestCase):
    def allowed(self, argv, launchers=None):
        return control.allowed_control_argv(
            argv, DISPLAY, control.DEFAULT_LAUNCHERS if launchers is None else launchers
        )

    def test_named_launchers_with_and_without_one_argument(self):
        self.assertTrue(self.allowed(LAUNCH + ["chrome"]))
        self.assertTrue(self.allowed(LAUNCH + ["chrome", "https://example.com/"]))
        self.assertTrue(self.allowed(LAUNCH + ["terminal"]))
        self.assertTrue(self.allowed(LAUNCH + ["files", "/home/pm"]))
        self.assertTrue(self.allowed(LAUNCH + ["editor", "/home/pm/notes.txt"]))

    def test_unknown_names_and_raw_paths_are_refused(self):
        self.assertFalse(self.allowed(LAUNCH + ["sh"]))
        self.assertFalse(self.allowed(LAUNCH + ["/usr/bin/google-chrome"]))
        self.assertFalse(self.allowed(LAUNCH + ["browser-legacy"]))
        self.assertFalse(self.allowed(LAUNCH + ["Chrome"]))

    def test_arguments_that_look_like_flags_or_are_empty_are_refused(self):
        self.assertFalse(self.allowed(LAUNCH + ["chrome", "--remote-debugging-port=9222"]))
        self.assertFalse(self.allowed(LAUNCH + ["chrome", "--user-data-dir=/tmp/x"]))
        self.assertFalse(self.allowed(LAUNCH + ["chrome", ""]))
        self.assertFalse(self.allowed(LAUNCH + ["chrome", "a", "b"]))
        self.assertFalse(self.allowed(LAUNCH))

    def test_browser_profile_prefix_only_for_browser_launchers(self):
        self.assertTrue(self.allowed(["env", f"DISPLAY={DISPLAY}", PROFILE_ARG, FOCUS, "chrome", "https://x/"]))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", PROFILE_ARG, FOCUS, "terminal"]))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", PROFILE_ARG, "xdotool", "mouseup", "1"]))

    def test_browser_profile_path_must_match_the_bot_profile_shape(self):
        bad = [
            "/home/other/.browser-profiles/chromium-bot-" + "a" * 32,
            "/home/pm/.browser-profiles/chromium-bot-" + "a" * 31,
            "/home/pm/.browser-profiles/chromium-bot-" + "g" * 32,
            "/home/pm/.browser-profiles/chromium-bot-" + "a" * 32 + "/",
            "/home/pm/../../etc",
            "/home/pm/.browser-profiles/chromium-bot-" + "a" * 32 + " ",
        ]
        for profile in bad:
            with self.subTest(profile=profile):
                argv = ["env", f"DISPLAY={DISPLAY}", f"PM_SBX_BROWSER_PROFILE={profile}", FOCUS, "chrome"]
                self.assertFalse(self.allowed(argv))

    def test_other_commands_are_refused(self):
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", "xdg-open", "https://x"]))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", "/bin/sh", "-c", "id"]))
        self.assertFalse(self.allowed(["env", f"DISPLAY={DISPLAY}", "xterm"]))


class BuildAndRunTests(unittest.TestCase):
    def test_chrome_launch_argv_has_no_shell_and_passes_profile_and_wait(self):
        settings = settings_for()
        argv = ["env", f"DISPLAY={DISPLAY}", PROFILE_ARG, FOCUS, "chrome", "https://example.com/"]
        command = control.build_launch_argv(argv, settings)
        self.assertEqual(
            command,
            [
                sys.executable, "-I", "/opt/pm-sbx/pm-sbx-focus-or-launch",
                "--wm-class", "google-chrome",
                "--profile", PROFILE,
                "--wait", "2.4",
                "--", "/usr/bin/google-chrome", "https://example.com/",
            ],
        )
        self.assertNotIn("sh", command)
        self.assertNotIn("-c", command)

    def test_terminal_launch_has_no_profile_or_wait(self):
        command = control.build_launch_argv(LAUNCH + ["terminal"], settings_for())
        self.assertEqual(
            command,
            [sys.executable, "-I", "/opt/pm-sbx/pm-sbx-focus-or-launch",
             "--wm-class", "xterm", "--", "/usr/bin/xterm"],
        )

    def test_child_env_pins_sandbox_display_rules(self):
        with mock.patch.dict(os.environ, {"WAYLAND_DISPLAY": "wayland-0", "DISPLAY": ":5"}, clear=False):
            env = control.child_env(DISPLAY)
        self.assertNotIn("WAYLAND_DISPLAY", env)
        self.assertEqual(env["DISPLAY"], DISPLAY)
        self.assertEqual(env["XDG_RUNTIME_DIR"], "/tmp/pm-runtime")
        self.assertEqual(env["GDK_BACKEND"], "x11")
        self.assertEqual(env["QT_QPA_PLATFORM"], "xcb")

    def test_xdotool_runs_as_argv_list_without_shell(self):
        completed = subprocess.CompletedProcess([], 0, b"", b"")
        with mock.patch.object(control.subprocess, "run", return_value=completed) as run:
            control.run_control_argv(xdotool("mouseup", "1"), DISPLAY, settings_for())
        args, kwargs = run.call_args
        self.assertEqual(args[0], xdotool("mouseup", "1"))
        self.assertNotIn("shell", kwargs)
        self.assertEqual(kwargs["env"]["DISPLAY"], DISPLAY)
        self.assertEqual(kwargs["timeout"], control.CONTROL_TIMEOUT_SEC)

    def test_launch_uses_the_wrapper_bound_and_fails_on_nonzero(self):
        ok = subprocess.CompletedProcess([], 0, b"", b"")
        bad = subprocess.CompletedProcess([], 3, b"", b"")
        settings = settings_for()
        argv = LAUNCH + ["editor"]
        with mock.patch.object(control.subprocess, "run", return_value=ok) as run:
            control.run_control_argv(argv, DISPLAY, settings)
        self.assertEqual(run.call_args.kwargs["timeout"], control.FOCUS_COMPLETION_SEC)
        self.assertEqual(run.call_args.args[0][-2:], ["--", "/usr/bin/mousepad"])
        with mock.patch.object(control.subprocess, "run", return_value=bad):
            with self.assertRaisesRegex(RuntimeError, "computer action failed"):
                control.run_control_argv(argv, DISPLAY, settings)

    def test_timeouts_become_action_errors(self):
        with mock.patch.object(
            control.subprocess, "run", side_effect=subprocess.TimeoutExpired("xdotool", 10)
        ):
            with self.assertRaisesRegex(RuntimeError, "timed out"):
                control.run_control_argv(xdotool("mouseup", "1"), DISPLAY, settings_for())


class ConfigTests(unittest.TestCase):
    def write_json(self, data):
        handle = tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8")
        with handle:
            if isinstance(data, str):
                handle.write(data)
            else:
                json.dump(data, handle)
        self.addCleanup(os.unlink, handle.name)
        return handle.name

    def test_defaults_when_no_file_is_configured(self):
        launchers = control.load_launchers("")
        self.assertEqual(set(launchers), {"chrome", "terminal", "files", "editor"})
        self.assertTrue(launchers["chrome"].browser_profile)
        self.assertFalse(launchers["terminal"].browser_profile)

    def test_json_file_replaces_the_allowlist(self):
        path = self.write_json({"launchers": {
            "notes": {"path": "/usr/bin/gedit", "wm_class": "gedit"},
            "web": {"path": "/usr/bin/firefox", "browser_profile": True},
        }})
        launchers = control.load_launchers(path)
        self.assertEqual(launchers["notes"], control.Launcher("/usr/bin/gedit", "gedit", False))
        self.assertEqual(launchers["web"], control.Launcher("/usr/bin/firefox", "firefox", True))
        self.assertNotIn("chrome", launchers)
        self.assertFalse(control.allowed_control_argv(LAUNCH + ["chrome"], DISPLAY, launchers))
        self.assertTrue(control.allowed_control_argv(LAUNCH + ["notes"], DISPLAY, launchers))

    def test_bad_launcher_files_stop_startup(self):
        bad_files = [
            "not json",
            {"other": {}},
            {"launchers": {}},
            {"launchers": {"Chrome": {"path": "/usr/bin/x"}}},
            {"launchers": {"chrome": {"path": "google-chrome"}}},  # relative path
            {"launchers": {"chrome": {"path": "/usr/bin/x", "wm_class": "-bad"}}},
            {"launchers": {"chrome": {"path": "/usr/bin/x", "browser_profile": "yes"}}},
            {"launchers": {"chrome": "/usr/bin/x"}},
        ]
        for data in bad_files:
            with self.subTest(data=data):
                with self.assertRaises(control.ConfigError):
                    control.load_launchers(self.write_json(data))
        with self.assertRaises(control.ConfigError):
            control.load_launchers(os.path.join(HERE, "does-not-exist.json"))

    def test_token_file_is_read_stripped_and_checked(self):
        path = self.write_json("  " + TOKEN + "\n")
        self.assertEqual(control.read_token_file(path), TOKEN)
        with self.assertRaises(control.ConfigError):
            control.read_token_file("")
        with self.assertRaises(control.ConfigError):
            control.read_token_file(os.path.join(HERE, "no-such-token-file"))
        with self.assertRaises(control.ConfigError):
            control.read_token_file(self.write_json("short"))
        with self.assertRaises(control.ConfigError):
            control.read_token_file(self.write_json("two words in this token value"))

    def test_port_and_display_validation(self):
        self.assertEqual(control.parse_port("47801"), 47801)
        for bad in ("0", "65536", "abc", ""):
            with self.subTest(port=bad):
                with self.assertRaises(control.ConfigError):
                    control.parse_port(bad)
        self.assertTrue(control.valid_display(":99"))
        self.assertTrue(control.valid_display(":0"))
        for bad in ("99", ":", "", ":99;ls", ":99 ", None, 99):
            with self.subTest(display=bad):
                self.assertFalse(control.valid_display(bad))
        self.assertEqual(control.DEFAULT_PORT, 47801)
        self.assertEqual(control.DEFAULT_DISPLAY, ":99")


class AuthTests(unittest.TestCase):
    def test_correct_bearer_token_is_accepted(self):
        self.assertTrue(control.authorized("Bearer " + TOKEN, TOKEN))

    def test_missing_wrong_or_malformed_headers_are_refused(self):
        self.assertFalse(control.authorized("", TOKEN))
        self.assertFalse(control.authorized("Bearer " + TOKEN[:-1], TOKEN))
        self.assertFalse(control.authorized("Bearer " + TOKEN + "x", TOKEN))
        self.assertFalse(control.authorized(TOKEN, TOKEN))  # no scheme
        self.assertFalse(control.authorized("Basic " + TOKEN, TOKEN))
        self.assertFalse(control.authorized("Bearer", TOKEN))
        self.assertFalse(control.authorized("bearer " + TOKEN, TOKEN))

    def test_empty_server_token_denies_everything(self):
        self.assertFalse(control.authorized("Bearer ", ""))
        self.assertFalse(control.authorized("Bearer x", ""))

    def test_non_ascii_header_is_refused_without_raising(self):
        self.assertFalse(control.authorized("Bearer ü", TOKEN))
        self.assertFalse(control.authorized("Bearer \udcff", TOKEN))

    def test_comparison_is_constant_time_compare_digest(self):
        with mock.patch.object(control.hmac, "compare_digest", wraps=hmac.compare_digest) as compare:
            control.authorized("Bearer " + TOKEN, TOKEN)
        compare.assert_called_once()


class CaptureHelperTests(unittest.TestCase):
    def test_png_dimensions_come_from_the_ihdr_chunk(self):
        header = control.PNG_SIGNATURE + b"\x00\x00\x00\x0dIHDR" + (1280).to_bytes(4, "big") + (800).to_bytes(4, "big")
        self.assertEqual(control.png_size(header + b"\x08\x02\x00\x00\x00"), (1280, 800))

    def test_non_png_output_is_a_capture_error(self):
        with self.assertRaisesRegex(RuntimeError, "screen capture failed"):
            control.png_size(b"GIF89a" + b"\x00" * 20)

    def test_display_lock_is_per_display(self):
        self.assertIs(control.display_lock(":99"), control.display_lock(":99"))
        self.assertIsNot(control.display_lock(":99"), control.display_lock(":98"))

    def test_plan_validates_every_step_before_running_any(self):
        settings = settings_for()
        body = {"steps": [
            {"argv": xdotool("mouseup", "1")},
            {"argv": xdotool("windowactivate", "1")},
        ]}
        with self.assertRaisesRegex(control.RequestError, "unsupported"):
            control.plan_steps(body, settings)

    def test_plan_clamps_waits_and_settle(self):
        settings = settings_for()
        display, planned, settle, observe = control.plan_steps(
            {"steps": [{"waitMs": 99999}], "settleMs": -4, "observe": False}, settings
        )
        self.assertEqual((display, planned, settle, observe), (DISPLAY, [("wait", 5000)], 0, False))
        with self.assertRaises(control.RequestError):
            control.plan_steps({"steps": [{"waitMs": "500"}]}, settings)
        with self.assertRaises(control.RequestError):
            control.plan_steps({"steps": [{"waitMs": True}]}, settings)


class FocusWrapperTests(unittest.TestCase):
    def test_wm_class_matches_whole_components_case_insensitively(self):
        self.assertTrue(focus.class_matches("google-chrome.Google-chrome", "google-chrome"))
        self.assertTrue(focus.class_matches("google-chrome.Google-chrome", "GOOGLE-CHROME"))
        self.assertFalse(focus.class_matches("google-chrome.Google-chrome", "chrome"))
        self.assertFalse(focus.class_matches("xterm-256color.XTerm", "xterm-256"))

    def test_default_wm_class_is_the_lowercase_basename(self):
        self.assertEqual(focus.default_wm_class("/usr/bin/XTerm"), "xterm")
        self.assertEqual(focus.default_wm_class("pcmanfm"), "pcmanfm")

    def test_first_matching_row_wins_and_bad_rows_are_skipped(self):
        listing = "\n".join([
            "0x01 0 100 other.Other host title",
            "garbage",
            "0x02 0 200 xterm.XTerm host first",
            "0x03 0 300 xterm.XTerm host second",
        ])
        self.assertEqual(focus.matching_window(listing, "xterm"), "0x02")
        self.assertEqual(focus.matching_window(listing, "missing"), "")

    def test_browser_profile_limits_the_match(self):
        listing = "\n".join([
            "0x10 0 1 google-chrome.Google-chrome host one",
            "0x11 0 2 google-chrome.Google-chrome host two",
        ])
        owners = {"1": "/home/pm/.browser-profiles/chromium-bot-" + "b" * 32,
                  "2": "/home/pm/.browser-profiles/chromium-bot-" + "c" * 32}
        with mock.patch.object(focus, "window_profile", side_effect=lambda pid: owners.get(pid)):
            # No window belongs to the requested profile, so nothing is raised.
            self.assertEqual(focus.matching_window(listing, "google-chrome", PROFILE), "")
            other = "/home/pm/.browser-profiles/chromium-bot-" + "c" * 32
            self.assertEqual(focus.matching_window(listing, "google-chrome", other), "0x11")
            self.assertEqual(focus.matching_window(listing, "google-chrome"), "0x10")

    def test_profile_is_read_from_user_data_dir_in_cmdline(self):
        raw = b"google-chrome\x00--user-data-dir=/home/pm/x\x00https://a/\x00"
        with mock.patch("builtins.open", mock.mock_open(read_data=raw)):
            self.assertEqual(focus.window_profile("123"), "/home/pm/x")
        raw = b"google-chrome\x00--user-data-dir\x00/home/pm/y\x00"
        with mock.patch("builtins.open", mock.mock_open(read_data=raw)):
            self.assertEqual(focus.window_profile("123"), "/home/pm/y")
        self.assertIsNone(focus.window_profile("not-a-pid"))

    def test_argument_parsing_keeps_launcher_arguments_after_dashes(self):
        options = focus.build_parser().parse_args(
            ["--wm-class", "xterm", "--wait", "1.5", "--", "/usr/bin/xterm", "-e", "htop"]
        )
        self.assertEqual((options.wm_class, options.wait, options.launcher, options.args),
                         ("xterm", 1.5, "/usr/bin/xterm", ["-e", "htop"]))

    def test_no_window_launches_with_profile_flag_and_arguments(self):
        with mock.patch.object(focus, "run_launcher", return_value=None) as launch, \
                mock.patch.object(focus.subprocess, "run",
                                  return_value=subprocess.CompletedProcess([], 0, "", "")):
            with self.assertRaises(SystemExit) as stop:
                focus.main(["--profile", PROFILE, "--", "/usr/bin/google-chrome", "https://x/"])
        self.assertEqual(stop.exception.code, 0)
        launch.assert_called_once_with(
            "/usr/bin/google-chrome", [f"--user-data-dir={PROFILE}", "https://x/"], 0.2
        )

    def test_existing_window_with_arguments_forwards_then_activates(self):
        listing = "0x20 0 9 xterm.XTerm host title\n"
        with mock.patch.object(focus, "run_launcher", return_value=0) as launch, \
                mock.patch.object(focus, "activate", side_effect=SystemExit(0)) as activate, \
                mock.patch.object(focus.subprocess, "run",
                                  return_value=subprocess.CompletedProcess([], 0, listing, "")):
            with self.assertRaises(SystemExit):
                focus.main(["--", "/usr/bin/xterm", "arg"])
        launch.assert_called_once_with("/usr/bin/xterm", ["arg"], 0.2)
        activate.assert_called_once_with("0x20")

    def test_existing_window_without_arguments_only_raises_it(self):
        listing = "0x20 0 9 xterm.XTerm host title\n"
        with mock.patch.object(focus, "run_launcher") as launch, \
                mock.patch.object(focus, "activate", side_effect=SystemExit(0)) as activate, \
                mock.patch.object(focus.subprocess, "run",
                                  return_value=subprocess.CompletedProcess([], 0, listing, "")):
            with self.assertRaises(SystemExit):
                focus.main(["--", "/usr/bin/xterm"])
        launch.assert_not_called()
        activate.assert_called_once_with("0x20")

    def test_missing_launcher_binary_exits_127(self):
        with mock.patch.object(focus.subprocess, "Popen", side_effect=FileNotFoundError(2, "missing")), \
                mock.patch.object(focus.subprocess, "run",
                                  return_value=subprocess.CompletedProcess([], 0, "", "")), \
                mock.patch.object(focus.sys, "stderr", io.StringIO()):
            with self.assertRaises(SystemExit) as stop:
                focus.main(["--", "/usr/bin/nope"])
        self.assertEqual(stop.exception.code, 127)

    def test_live_launcher_is_not_waited_out(self):
        child = mock.Mock()
        child.wait.side_effect = subprocess.TimeoutExpired("x", 0.2)
        with mock.patch.object(focus.subprocess, "Popen", return_value=child) as popen:
            self.assertIsNone(focus.run_launcher("/usr/bin/xterm", [], 0.2))
        self.assertTrue(popen.call_args.kwargs["start_new_session"])
        self.assertIs(popen.call_args.kwargs["stdin"], subprocess.DEVNULL)


class HttpTests(unittest.TestCase):
    """End to end over loopback with capture and run mocked (no X server)."""

    @classmethod
    def setUpClass(cls):
        cls.settings = settings_for()
        cls.server = control.ThreadingHTTPServer(("127.0.0.1", 0), control.make_handler(cls.settings))
        cls.port = cls.server.server_address[1]
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()

    @classmethod
    def tearDownClass(cls):
        cls.server.shutdown()
        cls.server.server_close()

    def post(self, path, payload=None, auth=("Bearer " + TOKEN), raw=None):
        data = raw if raw is not None else json.dumps(payload or {}).encode("utf-8")
        headers = {"Content-Type": "application/json"}
        if auth is not None:
            headers["Authorization"] = auth
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.port}{path}", data=data, headers=headers, method="POST"
        )
        try:
            with urllib.request.urlopen(request, timeout=10) as response:
                return response.status, json.loads(response.read() or b"{}")
        except urllib.error.HTTPError as error:
            with error:
                return error.code, json.loads(error.read() or b"{}")

    def test_missing_or_wrong_token_is_401_and_nothing_runs(self):
        with mock.patch.object(control, "run_control_argv") as run:
            self.assertEqual(self.post("/v1/desktop", {"steps": []}, auth=None)[0], 401)
            self.assertEqual(self.post("/v1/desktop", {"steps": []}, auth="Bearer nope")[0], 401)
            self.assertEqual(self.post("/v1/screenshot", {}, auth="Bearer nope")[0], 401)
        run.assert_not_called()

    def test_unknown_path_is_404(self):
        self.assertEqual(self.post("/v1/other", {})[0], 404)

    def test_disallowed_step_is_400_and_earlier_good_steps_do_not_run(self):
        with mock.patch.object(control, "run_control_argv") as run:
            status, body = self.post("/v1/desktop", {"steps": [
                {"argv": xdotool("mouseup", "1")},
                {"argv": ["sh", "-c", "id"]},
            ]})
        self.assertEqual(status, 400)
        self.assertEqual(body, {"error": "unsupported computer action"})
        run.assert_not_called()

    def test_malformed_bodies_are_400(self):
        self.assertEqual(self.post("/v1/desktop", raw=b"{nope")[0], 400)
        self.assertEqual(self.post("/v1/desktop", raw=b"[1,2]")[0], 400)
        self.assertEqual(self.post("/v1/desktop", {"display": ":99;ls"})[0], 400)

    def test_valid_actions_run_in_order_and_return_the_observation(self):
        observation = {"image": "AAAA", "mimeType": "image/png", "width": 1280, "height": 800}
        with mock.patch.object(control, "run_control_argv") as run, \
                mock.patch.object(control, "capture", return_value=observation) as capture:
            status, body = self.post("/v1/desktop", {
                "steps": [
                    {"argv": xdotool("mouseup", "1")},
                    {"waitMs": 10},
                    {"argv": LAUNCH + ["terminal"]},
                ],
                "settleMs": 0,
            })
        self.assertEqual(status, 200)
        self.assertEqual(body, {"completed": 3, "observation": observation})
        self.assertEqual(run.call_count, 2)
        self.assertEqual(run.call_args_list[0].args[0], xdotool("mouseup", "1"))
        self.assertEqual(run.call_args_list[1].args[0], LAUNCH + ["terminal"])
        capture.assert_called_once_with(DISPLAY)

    def test_observe_false_skips_capture(self):
        with mock.patch.object(control, "run_control_argv"), \
                mock.patch.object(control, "capture") as capture:
            status, body = self.post("/v1/desktop", {"steps": [], "observe": False})
        self.assertEqual((status, body), (200, {"completed": 0}))
        capture.assert_not_called()

    def test_screenshot_endpoint_returns_an_observation(self):
        observation = {"image": "AAAA", "mimeType": "image/png", "width": 2, "height": 2}
        with mock.patch.object(control, "capture", return_value=observation):
            status, body = self.post("/v1/screenshot", {"display": DISPLAY})
        self.assertEqual((status, body), (200, {"observation": observation}))
        self.assertEqual(self.post("/v1/screenshot", {"display": "bad"})[0], 400)

    def test_action_failures_are_500_with_the_fixed_message(self):
        with mock.patch.object(control, "run_control_argv", side_effect=RuntimeError("computer action failed")):
            status, body = self.post("/v1/desktop", {"steps": [{"argv": xdotool("mouseup", "1")}]})
        self.assertEqual((status, body), (500, {"error": "computer action failed"}))


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Owned GTK3/X11 readiness receiver. Run only through the guarded native test."""
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

if sys.platform != "linux" or os.environ.get("ICU_DISPOSABLE_HOST") != "1":
    raise SystemExit("disposable Linux host required")
if os.environ.get("ICU_E2E_PORTABLE_READINESS") != "1":
    raise SystemExit("explicit portable readiness test required")
if os.environ.get("XDG_SESSION_TYPE") != "x11":
    raise SystemExit("this receiver proves GTK3/X11 only")
if Path("/Users/nathanzhao").exists() or "nathanzhao" in str(Path.home()).lower():
    raise SystemExit("never run on Nathan's working desktop")

import gi
gi.require_version("Gtk", "3.0")
gi.require_version("GdkX11", "3.0")
from gi.repository import Gtk, GLib, GdkX11

ROOT = Path(sys.argv[1]).resolve(strict=True)
MODE = sys.argv[2]
assert MODE in ("delayed", "permanent", "noeffect", "busy-static")


def foreground():
    result = subprocess.run(["xprop", "-root", "_NET_ACTIVE_WINDOW"],
                            capture_output=True, text=True, timeout=2, check=True)
    match = re.search(r"0x[0-9a-fA-F]+", result.stdout)
    if not match:
        raise RuntimeError("X11 active-window evidence unavailable")
    return int(match.group(), 16)


def write(name, value):
    temporary = ROOT / (name + ".tmp")
    temporary.write_text(json.dumps(value), encoding="utf-8")
    temporary.replace(ROOT / name)


class Receiver:
    def __init__(self):
        self.state = {"mode": MODE, "pid": os.getpid(), "target_effects": 0,
                      "decoy_effects": 0, "result": "0", "busy": False,
                      "ever_active": False, "foreground_windows": [], "events": [],
                      "ownership_samples": 0}
        self.initial_foreground = foreground()
        if not self.initial_foreground:
            raise RuntimeError("an unrelated foreground guard window is required")
        self.windows = []
        self.target = self.window("ICU Portable Readiness Target", True)
        self.decoy = self.window("ICU Portable Readiness Decoy", False)
        self.started = None
        self.tick = 0
        self.sample()
        write("ready.json", {"pid": os.getpid(),
              "target_window_id": self.target.get_window().get_xid(),
              "decoy_window_id": self.decoy.get_window().get_xid(),
              "front_window_id": self.initial_foreground, "toolkit": "gtk3-x11"})
        GLib.timeout_add(20, self.sample)
        GLib.timeout_add_seconds(35, self.stop)

    def window(self, title, target):
        window = Gtk.Window(title=title)
        window.set_default_size(420, 220)
        window.set_focus_on_map(False)
        window.set_accept_focus(False)
        window.move(80 if target else 540, 100)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=16)
        box.set_border_width(20)
        window.add(box)
        result = Gtk.Entry()
        result.set_text("0" if target else "decoy unchanged")
        result.get_accessible().set_name("Result")
        box.pack_start(result, False, False, 0)
        button = Gtk.Button(label="Compute")
        button.connect("clicked", lambda *_: self.compute(target))
        box.pack_start(button, False, False, 0)
        if target:
            self.result = result
            self.spinner = Gtk.Spinner()
            self.spinner.get_accessible().set_name("Loading result")
            box.pack_start(self.spinner, False, False, 0)
        window.connect("notify::is-active", self.activation)
        window.show_all()
        if target:
            self.spinner.hide()
        self.windows.append(window)
        return window

    def activation(self, window, _):
        self.state["ever_active"] |= bool(window.props.is_active)
        self.save()

    def save(self):
        write("state.json", self.state)

    def sample(self):
        try:
            current = foreground()
        except Exception as error:
            self.state["ownership_error"] = str(error)
            self.save()
            Gtk.main_quit()
            return False
        if current not in self.state["foreground_windows"]:
            self.state["foreground_windows"].append(current)
        self.state["ever_active"] |= any(w.props.is_active for w in self.windows)
        self.state["ownership_samples"] += 1
        self.state["ownership_sampled_at"] = time.time()
        self.save()
        return True

    def event(self, kind):
        self.state["events"].append({"kind": kind, "at": time.time()})
        self.save()

    def compute(self, target):
        key = "target_effects" if target else "decoy_effects"
        self.state[key] += 1
        if not target or MODE == "noeffect":
            self.event("target_noeffect" if target else "decoy_compute")
            return
        self.started = time.monotonic()
        self.state["started_at"] = time.time()
        self.state["busy"] = True
        self.result.set_text("Working")
        self.state["result"] = "Working"
        self.spinner.show()
        self.spinner.start()
        self.event("target_started")
        if MODE != "busy-static":
            GLib.timeout_add(30, self.update)

    def update(self):
        if MODE == "delayed" and time.monotonic() - self.started >= 1.0:
            self.result.set_text("42")
            self.state.update(result="42", busy=False, completed_at=time.time())
            self.spinner.stop()
            self.spinner.hide()
            self.event("target_completed")
            return False
        self.tick += 1
        self.state["result"] = "Working " + str(self.tick)
        self.result.set_text(self.state["result"])
        self.save()
        return True

    def stop(self):
        self.event("fixture_expired")
        Gtk.main_quit()
        return False


receiver = Receiver()
Gtk.main()

#!/usr/bin/env python3
"""Isolated Linux GUI smoke. Requires Xvfb, xdotool, ImageMagick, cargo build.
Run: python3 scripts/native-smoke.py
All keyboard input is confined to a new Xvfb display; notes/config use a temp dir.
"""
import ctypes as c
import hashlib
import json
import os
from pathlib import Path
import subprocess as sp
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
# Override to exercise the exact release/package/installed binary without copying it.
BINARY = Path(os.environ.get("VELOCIMD_SMOKE_BINARY", ROOT / "target/debug/velocimd")).resolve()


def run(*args, **kwargs):
    return sp.check_output(args, text=True, timeout=15, **kwargs).strip()


def wait_for(predicate, label, seconds=8):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.05)
    raise AssertionError(f"Timed out: {label}")


def close_window(window):
    """Send WM_DELETE_WINDOW directly; no window manager is required in Xvfb."""
    class Data(c.Union):
        _fields_ = [("b", c.c_char * 20), ("s", c.c_short * 10), ("l", c.c_long * 5)]

    class Client(c.Structure):
        _fields_ = [("type", c.c_int), ("serial", c.c_ulong), ("send_event", c.c_int),
                    ("display", c.c_void_p), ("window", c.c_ulong),
                    ("message_type", c.c_ulong), ("format", c.c_int), ("data", Data)]

    class Event(c.Union):
        _fields_ = [("client", Client), ("pad", c.c_long * 24)]

    lib = c.CDLL("libX11.so.6")
    lib.XOpenDisplay.argtypes = [c.c_char_p]
    lib.XOpenDisplay.restype = c.c_void_p
    lib.XInternAtom.argtypes = [c.c_void_p, c.c_char_p, c.c_int]
    lib.XInternAtom.restype = c.c_ulong
    lib.XSendEvent.argtypes = [c.c_void_p, c.c_ulong, c.c_int, c.c_long, c.POINTER(Event)]
    lib.XFlush.argtypes = [c.c_void_p]
    lib.XCloseDisplay.argtypes = [c.c_void_p]
    display = lib.XOpenDisplay(None)
    assert display
    try:
        event = Event()
        event.client.type = 33
        event.client.send_event = 1
        event.client.display = display
        event.client.window = int(window)
        event.client.message_type = lib.XInternAtom(display, b"WM_PROTOCOLS", 0)
        event.client.format = 32
        event.client.data.l[0] = lib.XInternAtom(display, b"WM_DELETE_WINDOW", 0)
        assert lib.XSendEvent(display, int(window), 0, 0, c.byref(event)) != 0
        lib.XFlush(display)
    finally:
        lib.XCloseDisplay(display)


def main():
    if os.environ.get("VELOCIMD_SMOKE_XVFB") != "1":
        env = dict(os.environ, VELOCIMD_SMOKE_XVFB="1")
        return sp.call(["xvfb-run", "-a", "-s", "-screen 0 1280x900x24", sys.executable, __file__], env=env)
    output = ROOT / "target/native-smoke"
    output.mkdir(parents=True, exist_ok=True)
    (output / "result.json").unlink(missing_ok=True)
    screenshots = []
    processes = []
    with tempfile.TemporaryDirectory(prefix="velocimd-native-smoke-") as scratch:
        scratch = Path(scratch)
        note = scratch / "Smoke.MD"
        content = "# Native smoke\n\n" + "Wrapped markdown text. " * 160 + "\n\nLast paragraph."
        note.write_text(content)
        config = scratch / "config"
        session = config / "velocimd/state.json"
        env = dict(os.environ, XDG_CONFIG_HOME=str(config), LIBGL_ALWAYS_SOFTWARE="1",
                   GALLIUM_DRIVER="llvmpipe", WINIT_UNIX_BACKEND="x11", NO_AT_BRIDGE="1",
                   DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent")
        env.pop("WAYLAND_DISPLAY", None)

        def start(*files):
            process = sp.Popen([str(BINARY), *map(str, files)], env=env,
                               stdout=log, stderr=log)
            processes.append(process)
            window = run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(process.pid)).splitlines()[0]
            assert Path(f"/proc/{process.pid}/exe").samefile(BINARY), "smoke launched the wrong binary"
            run("xdotool", "windowsize", "--sync", window, "1100", "800")
            run("xdotool", "windowfocus", "--sync", window)
            return process, window

        def key(*keys):
            run("xdotool", "key", "--clearmodifiers", "--delay", "80", *keys)

        def capture(window, name):
            path = output / name
            run("import", "-window", window, str(path))
            assert path.stat().st_size > 1000
            screenshots.append(str(path))

        def mode_is(mode):
            try:
                return json.loads(session.read_text())["mode"] == mode
            except (FileNotFoundError, json.JSONDecodeError):
                return False

        with (output / "app.log").open("w") as log:
            try:
                process, window = start(note)
                key("ctrl+k")
                run("xdotool", "type", "--clearmodifiers", "--delay", "80", "preview")
                capture(window, "palette.png")
                key("Return")
                wait_for(lambda: mode_is("Preview"), "palette changes mode")
                capture(window, "preview.png")
                key("ctrl+1")
                wait_for(lambda: mode_is("Edit"), "editor mode")
                run("xdotool", "mousemove", "--window", window, "250", "240", "click", "1")
                key("ctrl+End")
                marker = "\nNative keyboard autosave verified."
                key("Return")
                run("xdotool", "type", "--clearmodifiers", "--delay", "40", marker[1:])
                capture(window, "editor-attempt.png")
                try:
                    wait_for(lambda: note.read_text() == content + marker, "native typing autosaves exact content")
                except AssertionError:
                    actual = note.read_text()
                    import difflib
                    expected = content + marker
                    edits = [(tag, repr(expected[i:j]), repr(actual[k:l])) for tag, i, j, k, l in difflib.SequenceMatcher(None, expected, actual, autojunk=False).get_opcodes() if tag != "equal"]
                    print(json.dumps({"edits": edits, "expected_suffix": marker, "actual_suffix": actual[-220:], "original_chars": len(content), "actual_chars": len(actual)}))
                    capture(window, "editor-failure.png")
                    if session.exists():
                        (output / "failure-state.json").write_text(session.read_text())
                    raise
                capture(window, "editor.png")
                key("ctrl+3")
                wait_for(lambda: mode_is("Split"), "split mode")
                capture(window, "split.png")
                close_window(window)
                assert process.wait(timeout=8) == 0
                state = json.loads(session.read_text())
                assert state["documents"][state["active_document"]]["path"] == str(note)
                assert not state["command_palette_open"]
                process, window = start()
                key("ctrl+k", "Escape", "ctrl+s")
                wait_for(lambda: note.read_text() == content + marker, "restart preserves saved note")
                close_window(window)
                assert process.wait(timeout=8) == 0
            finally:
                for process in processes:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=5)
        with BINARY.open("rb") as binary_file:
            binary_sha256 = hashlib.file_digest(binary_file, "sha256").hexdigest()
        result = {"passed": True, "binary": str(BINARY), "binary_sha256": binary_sha256, "checks": ["native startup", "palette search/Enter", "mode persistence",
                  "keyboard editing", "debounced autosave", "graceful close", "restart/reopen"],
                  "screenshots": screenshots, "exit_codes": [p.returncode for p in processes]}
        (output / "result.json").write_text(json.dumps(result, indent=2))
        print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())

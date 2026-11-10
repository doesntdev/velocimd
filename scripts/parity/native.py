#!/usr/bin/env python3
"""Actual Linux app capture/interaction in a private Xvfb, never the user display."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess as sp
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[2]
CONTRACT = json.loads((ROOT / "tests/parity/contract.json").read_text())


def sha(path):
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def wait_for(predicate, label, seconds=12):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        value = predicate()
        if value:
            return value
        time.sleep(0.06)
    raise AssertionError(f"Timed out: {label}")


class NativeSession:
    def __init__(self, binary, output, *, probe=True):
        self.binary = Path(binary).resolve(strict=True)
        self.output = Path(output).resolve()
        self.output.mkdir(parents=True, exist_ok=False)
        base = os.environ.get("TMPDIR") or str(ROOT / "target/parity/tmp")
        Path(base).mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="velocimd-parity-", dir=base)
        self.scratch = Path(self.temp.name)
        self.probe = probe
        self.process = self.xvfb = None
        self.log = (self.output / "app.log").open("w")
        self.xlog = (self.output / "xvfb.log").open("w")
        self.viewport = CONTRACT["viewport"]
        self.fixture = (ROOT / "tests/parity/fixture.md").read_text()
        self.note = self.scratch / "parity.md/parity.md"
        self.note.parent.mkdir()
        self.note.write_text(self.fixture)
        self.original_note_sha = sha(self.note)
        self.session = self.scratch / "config/velocimd/state.json"
        self.session.parent.mkdir(parents=True)
        self.session.write_text(json.dumps({
            "documents": [{"id": 1, "title": "parity.md", "path": str(self.note), "dirty": False}],
            "active_document": 0, "folder_tabs": [{"path": str(self.note.parent)}],
            "active_folder_tab": 0, "mode": "Split", "split_ratio": 0.5,
            "working_folder": str(self.note.parent),
            "window_size": [self.viewport["width"], self.viewport["height"]],
        }))
        self.trace_path = self.scratch / "ui-probe.json"
        self.env = dict(os.environ, XDG_CONFIG_HOME=str(self.scratch / "config"),
                        XDG_DATA_HOME=str(self.scratch / "data"), XDG_CACHE_HOME=str(self.scratch / "cache"),
                        XDG_STATE_HOME=str(self.scratch / "state"), HOME=str(self.scratch / "home"),
                        LIBGL_ALWAYS_SOFTWARE="1", GALLIUM_DRIVER="llvmpipe", WINIT_UNIX_BACKEND="x11",
                        WINIT_X11_SCALE_FACTOR="1", NO_AT_BRIDGE="1", GDK_BACKEND="x11",
                        GTK_USE_PORTAL="0", GSETTINGS_BACKEND="memory",
                        DBUS_SESSION_BUS_ADDRESS=f"unix:path={self.scratch}/absent-dbus")
        for name in ("WAYLAND_DISPLAY", "VELOCIMD_UI_PROBE_PATH"):
            self.env.pop(name, None)
        Path(self.env["HOME"]).mkdir()
        if probe:
            self.env["VELOCIMD_UI_PROBE_PATH"] = str(self.trace_path)

    def run(self, *args, timeout=15):
        return sp.check_output(args, env=self.env, text=True, stderr=sp.STDOUT, timeout=timeout).strip()

    def __enter__(self):
        try:
            self.xvfb = sp.Popen(["Xvfb", "-displayfd", "1", "-screen", "0", "1500x1000x24", "-nolisten", "tcp"],
                                 stdout=sp.PIPE, stderr=self.xlog, text=True)
            assert self.xvfb.stdout is not None
            import select
            assert select.select([self.xvfb.stdout], [], [], 8)[0], "Xvfb display allocation timed out"
            display = self.xvfb.stdout.readline().strip()
            assert display.isdigit(), "Xvfb did not allocate an owned display"
            self.env["DISPLAY"] = ":" + display
            assert self.env["DISPLAY"] != os.environ.get("DISPLAY"), "Refuse user's display"
            self.process = sp.Popen([str(self.binary)], env=self.env, stdout=self.log, stderr=self.log)
            self.window = self.run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(self.process.pid)).splitlines()[0]
            assert Path(f"/proc/{self.process.pid}/exe").samefile(self.binary)
            assert sha(f"/proc/{self.process.pid}/exe") == sha(self.binary)
            self.run("xdotool", "windowsize", "--sync", self.window, str(self.viewport["width"]), str(self.viewport["height"]))
            self.run("xdotool", "windowfocus", "--sync", self.window)
            if self.probe:
                wait_for(lambda: self.snapshot_or_none(), "actual native frame probe")
            else:
                time.sleep(0.8)
            return self
        except BaseException:
            self.close()
            raise

    def snapshot_or_none(self):
        try:
            value = json.loads(self.trace_path.read_text())
            return value if value.get("schema") == 1 and value.get("controls") else None
        except (FileNotFoundError, json.JSONDecodeError):
            return None

    def snapshot(self):
        return wait_for(self.snapshot_or_none, "read native snapshot")

    def key(self, *keys):
        self.run("xdotool", "key", "--clearmodifiers", "--delay", "60", *keys)

    def type_text(self, text):
        for index, line in enumerate(text.split("\n")):
            if index:
                self.key("Return")
            if line:
                self.run("xdotool", "type", "--clearmodifiers", "--delay", "1", "--", line)

    def click(self, control_id):
        controls = [item for item in self.snapshot()["controls"] if item["id"] == control_id]
        assert len(controls) == 1, f"Missing/ambiguous native control: {control_id}"
        control = controls[0]
        assert control["enabled"], f"Disabled native control: {control_id}"
        self.click_rect(control["rect"])

    def click_rect(self, rect):
        x, y, width, height = rect
        self.run("xdotool", "mousemove", "--window", self.window, str(round(x + width / 2)), str(round(y + height / 2)), "click", "1")

    def neutral_pointer(self):
        self.run("xdotool", "mousemove", "--window", self.window, "1100", "18", "click", "1")
        self.run("xdotool", "mousemove", "1450", "950")

    def mode_is(self, mode):
        if self.probe:
            data = self.snapshot_or_none()
            return data and data.get("mode") == mode
        try:
            return json.loads(self.session.read_text()).get("mode") == mode
        except (FileNotFoundError, json.JSONDecodeError):
            return False

    def capture(self, case):
        self.key("alt+l" if case["light"] else "alt+d")
        self.key({"Edit": "ctrl+1", "Preview": "ctrl+2", "Split": "ctrl+3"}[case["mode"]])
        wait_for(lambda: self.mode_is(case["mode"]), "native mode selection")
        if self.probe:
            wait_for(lambda: self.snapshot().get("light") == case["light"], "native theme selection")
        self.neutral_pointer()
        path = self.output / (case["id"] + ".png")
        previous = None
        stable_since = None
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            self.run("import", "-window", self.window, "-strip", "PNG24:" + str(path))
            digest = sha(path)
            now = time.monotonic()
            if digest == previous and path.stat().st_size > 6000:
                stable_since = stable_since or now
                if now - stable_since >= 0.3:
                    result = {"id": case["id"], "image": str(path), "imageSha256": digest, "expected": case}
                    if self.probe:
                        result["snapshot"] = self.snapshot()
                    return result
            else:
                stable_since = None
            previous = digest
            time.sleep(0.12)
        raise AssertionError(f"Native image did not settle: {case['id']}")

    def click_editor(self):
        x, y, width, height = self.snapshot()["regions"]["editor"]
        self.run("xdotool", "mousemove", "--window", self.window,
                 str(round(x + min(100, width / 2))), str(round(y + min(25, height / 2))), "click", "1")

    def choose_file(self, control_id, path, *, expected_title):
        self.click(control_id)

        def dialog():
            try:
                windows = self.run("xdotool", "search", "--onlyvisible", "--name", ".+").splitlines()
                return next((window for window in windows if window != self.window), None)
            except sp.CalledProcessError:
                return None

        chooser = wait_for(dialog, "native file chooser")
        self.run("xdotool", "windowfocus", "--sync", chooser)
        # GTK maps before its initial folder loading and entry focus are ready.
        time.sleep(1.0)
        self.key("ctrl+l")
        time.sleep(0.15)
        self.key("ctrl+a")
        self.type_text(str(path))
        self.key("Return")
        try:
            wait_for(lambda: (self.snapshot_or_none() or {}).get("active_title") == expected_title,
                     "native chooser completion", seconds=2)
        except AssertionError:
            if dialog() == chooser:
                self.key("Return")
            wait_for(lambda: (self.snapshot_or_none() or {}).get("active_title") == expected_title,
                     "native chooser completion")
        self.run("xdotool", "windowfocus", "--sync", self.window)

    def exercise(self):
        checks = []
        for control, mode in (("edit", "Edit"), ("preview", "Preview"), ("split", "Split"), ("edit", "Edit")):
            self.click("toolbar:" + control)
            wait_for(lambda: self.mode_is(mode), "actual mode control " + mode)
        assert self.snapshot()["active_content"] == self.fixture
        checks.append({"id": "actual-mode-controls-preserve-document", "status": "passed"})
        before = self.snapshot()["light"]
        self.click("toolbar:theme")
        wait_for(lambda: self.snapshot()["light"] != before, "actual theme control")
        assert self.snapshot()["active_content"] == self.fixture
        checks.append({"id": "actual-theme-control-preserves-document", "status": "passed"})
        self.click("toolbar:new")
        wait_for(lambda: self.snapshot()["active_title"] != "parity.md", "new file control")
        self.click("filename")
        self.key("ctrl+a")
        self.type_text("parity-created")
        self.key("Return")
        wait_for(lambda: self.snapshot()["active_title"] == "parity-created.md", "filename commit")
        self.click_editor()
        self.key("ctrl+a")
        content = "# Native parity\n\nActual shared editor input."
        self.type_text(content)
        wait_for(lambda: self.snapshot()["active_content"] == content, "native exact editor bytes")
        note = self.note.parent / "parity-created.md"

        def saved():
            return note.exists() and note.read_text() == content

        self.click("toolbar:save")
        wait_for(saved, "native disk save")
        checks.append({"id": "new-rename-real-editor-input-and-disk-save", "status": "passed"})
        # Verify the undo route on one isolated real keystroke, not a synthetic state setter.
        self.click_editor()
        self.key("ctrl+End")
        # Cursor position belongs to the undo snapshot too. Stabilize after moving it.
        time.sleep(1.5)
        self.run("xdotool", "mousemove", "1450", "950")
        time.sleep(0.12)
        self.type_text("Z")
        wait_for(lambda: self.snapshot()["active_content"] == content + "Z", "native undo input")
        self.key("ctrl+z")
        wait_for(lambda: self.snapshot()["active_content"] == content, "native undo")
        self.key("ctrl+shift+z")
        wait_for(lambda: self.snapshot()["active_content"] == content + "Z", "native redo")
        self.key("ctrl+z")
        wait_for(saved, "native undo saved bytes")
        checks.append({"id": "undo-redo-through-real-input", "status": "passed"})
        self.click("toolbar:split")
        wait_for(lambda: self.mode_is("Split"), "native split")
        snapshot = self.snapshot()
        x, y, width, height = snapshot["regions"]["divider"]
        self.run("xdotool", "mousemove", "--window", self.window, str(round(x+width/2)), str(round(y+height/2)), "mousedown", "1")
        self.run("xdotool", "mousemove", "--sync", "--window", self.window, str(round(x+width/2+130)), str(round(y+height/2)))
        self.run("xdotool", "mouseup", "1")
        wait_for(lambda: self.snapshot()["split_ratio"] > snapshot["split_ratio"] + 0.05, "native divider movement")
        checks.append({"id": "split-divider-pointer-input", "status": "passed"})
        if self.snapshot()["light"]:
            self.click("toolbar:theme")
        self.neutral_pointer()
        self.key("ctrl+k")
        self.type_text("Switch to light theme")
        self.key("Return")
        wait_for(lambda: self.snapshot()["light"], "native palette command")
        assert self.snapshot()["active_content"] == content
        checks.append({"id": "palette-search-executes-without-source-mutation", "status": "passed"})
        opened = self.note.parent / "opened.md"
        opened_content = "# Native open\n\nOwned fixture.\n"
        opened.write_text(opened_content)
        self.choose_file("toolbar:open-file", opened, expected_title="opened.md")
        assert self.snapshot()["active_content"] == opened_content
        checks.append({"id": "actual-open-file-chooser", "status": "passed"})
        copy = self.note.parent / "copied.md"
        self.choose_file("toolbar:save-as", copy, expected_title="copied.md")
        wait_for(lambda: copy.exists() and copy.read_text() == opened_content, "native Save As bytes")
        assert opened.read_text() == opened_content
        checks.append({"id": "actual-save-as-chooser-retains-original", "status": "passed"})
        assert sha(self.note) == self.original_note_sha
        assert note.read_text() == content
        return checks

    def close(self):
        exit_code = None
        if self.process and self.process.poll() is None:
            try:
                code = ("import importlib.util,sys; s=importlib.util.spec_from_file_location('native_smoke',sys.argv[1]); "
                        "m=importlib.util.module_from_spec(s);s.loader.exec_module(m);m.close_window(sys.argv[2])")
                sp.run([sys.executable, "-c", code, str(ROOT / "scripts/native-smoke.py"), self.window],
                       env=self.env, stdout=self.log, stderr=self.log, check=True, timeout=10)
                exit_code = self.process.wait(timeout=12)
            except (Exception, KeyboardInterrupt):
                self.process.terminate()
                try:
                    exit_code = self.process.wait(timeout=5)
                except sp.TimeoutExpired:
                    self.process.kill()
                    exit_code = self.process.wait(timeout=5)
        elif self.process:
            exit_code = self.process.returncode
        if self.xvfb and self.xvfb.poll() is None:
            self.xvfb.terminate()
            try:
                self.xvfb.wait(timeout=5)
            except sp.TimeoutExpired:
                self.xvfb.kill()
                self.xvfb.wait(timeout=5)
        self.log.close()
        self.xlog.close()
        self.temp.cleanup()
        return exit_code

    def __exit__(self, *exc):
        if exc[0] is not None:
            snapshot = self.snapshot_or_none()
            if snapshot:
                (self.output / "failure-snapshot.json").write_text(json.dumps(snapshot, indent=2) + "\n")
            try:
                tree = self.run("xwininfo", "-root", "-tree")
                (self.output / "failure-window-tree.txt").write_text(tree + "\n")
                self.run("import", "-window", self.window, "-strip", "PNG24:" + str(self.output / "failure.png"))
            except Exception:
                pass
        self.exit_code = self.close()
        if exc[0] is None:
            assert self.exit_code == 0, f"Native app exit was {self.exit_code}"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--reference", action="store_true")
    args = parser.parse_args()
    if args.reference:
        assert sha(args.binary) == CONTRACT["reference"]["nativeBinarySha256"], "Wrong native reference binary"
    output = Path(args.output)
    report_path = output.parent / (output.name + "-report.json")
    assert not report_path.exists(), "Refuse to overwrite a prior native report"
    report = {"schema": 1, "status": "running", "reference": args.reference,
              "binarySha256": sha(args.binary), "fixtureSha256": sha(ROOT / "tests/parity/fixture.md"),
              "viewport": CONTRACT["viewport"], "cases": [], "checks": []}
    try:
        with NativeSession(args.binary, output, probe=not args.reference) as app:
            for case in CONTRACT["cases"]:
                report["cases"].append(app.capture(case))
            assert sha(app.note) == app.original_note_sha, "Capture mutated the fixture document"
            report["checks"].append({"id": "capture-preserves-document", "status": "passed"})
            if not args.reference:
                report["checks"].extend(app.exercise())
        report["exitCode"] = app.exit_code
        report["status"] = "passed"
    except BaseException as error:
        report["status"] = "failed"
        report["error"] = str(error)
        raise
    finally:
        report_path.write_text(json.dumps(report, indent=2) + "\n")
        print(json.dumps({"status": report["status"], "cases": len(report["cases"]), "report": str(report_path)}))


if __name__ == "__main__":
    main()

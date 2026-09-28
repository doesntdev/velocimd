#!/usr/bin/env python3
"""Native toolbar review. Requires native-smoke.py's tools plus Pillow.

Run after cargo build. Uses only Xvfb, disposable notes, and disposable settings.
Writes native screenshots and a source-art contact sheet to target/icon-review.
"""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess as sp
import sys
import tempfile
import time

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
sys.dont_write_bytecode = True
SPEC = importlib.util.spec_from_file_location("native_smoke", ROOT / "scripts/native-smoke.py")
assert SPEC is not None and SPEC.loader is not None
SMOKE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(SMOKE)
OUTPUT = ROOT / "target/icon-review"


def source_sheet():
    """An enlarged source-art review, clearly separate from the native captures."""
    names = [
        ("file-plus", "New note"), ("folder", "Open folder"), ("file", "Open file"),
        ("save", "Save"), ("save-as", "Save as"), ("close", "Close"),
        ("edit", "Edit"), ("preview", "Preview"), ("split", "Split view"),
        ("cycle", "Cycle mode"), ("sun", "Light"), ("moon", "Dark"),
        ("plus", "Add folder"), ("check", "Saved"), ("command", "Commands"),
    ]
    font_path = "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"
    title_font = ImageFont.truetype(font_path, 24)
    label_font = ImageFont.truetype(font_path, 13)
    small_font = ImageFont.truetype(font_path, 12)
    sheet = Image.new("RGB", (1020, 590))
    draw = ImageDraw.Draw(sheet)
    for theme, y, background, color, muted in [
        ("Velocidark", 0, "#12141b", "#dde1ea", "#8f97a6"),
        ("Velocilight", 295, "#ebedf2", "#1c2028", "#626b7c"),
    ]:
        draw.rectangle((0, y, 1020, y + 295), fill=background)
        draw.text((28, y + 18), f"Velocimd / {theme}", font=title_font, fill=color)
        draw.text((28, y + 54), "Toolbar icon family · enlarged source art", font=small_font, fill=muted)
        for index, (name, label) in enumerate(names):
            png = OUTPUT / f"source-{name}.png"
            if y == 0:
                SMOKE.run("convert", "-background", "none", "-density", "192",
                          str(ROOT / "assets/icons/toolbar" / f"{name}.svg"),
                          "-resize", "48x48", str(png))
            with Image.open(png) as image:
                alpha = image.convert("RGBA").getchannel("A")
                tinted = Image.new("RGBA", alpha.size, color)
                tinted.putalpha(alpha)
                x = 28 + (index % 8) * 124
                row_y = y + 92 + (index // 8) * 97
                sheet.paste(tinted, (x + 22, row_y), tinted)
                draw.text((x + 46, row_y + 56), label, font=label_font, fill=muted, anchor="mt")
    path = OUTPUT / "icon-family.png"
    sheet.save(path)
    return str(path)


def main():
    if os.environ.get("VELOCIMD_ICON_XVFB") != "1":
        env = dict(os.environ, VELOCIMD_ICON_XVFB="1")
        return sp.call(["xvfb-run", "-a", "-s", "-screen 0 2560x1800x24 -nolisten tcp",
                        sys.executable, __file__], env=env)
    OUTPUT.mkdir(parents=True, exist_ok=True)
    result_file = OUTPUT / "result.json"
    result_file.unlink(missing_ok=True)
    screenshots = []
    exits = []
    with tempfile.TemporaryDirectory(prefix="velocimd-icons-") as scratch:
        scratch = Path(scratch)
        notes = scratch / "Notes"
        notes.mkdir()
        note = notes / "Quiet workspace.md"
        content = ("# A little room to think\n\n"
                   "Plain text. Clear ideas. Nothing in the way.\n\n"
                   "## Today\n\n- [x] Find a quieter visual language\n"
                   "- [x] Keep the tools familiar\n- [ ] Write something worth keeping\n\n"
                   "> Good tools leave room for the work.\n\n"
                   "## The next thought\n\nA small note can be the beginning of something larger.\n")
        note.write_text(content)
        for scale in [1, 2]:
            config = scratch / f"config-{scale}"
            session = config / "velocimd/state.json"
            env = dict(os.environ, XDG_CONFIG_HOME=str(config),
                       LIBGL_ALWAYS_SOFTWARE="1", GALLIUM_DRIVER="llvmpipe",
                       WINIT_UNIX_BACKEND="x11", WINIT_X11_SCALE_FACTOR=str(scale),
                       NO_AT_BRIDGE="1", DBUS_SESSION_BUS_ADDRESS="unix:path=/nonexistent")
            env.pop("WAYLAND_DISPLAY", None)
            with (OUTPUT / f"app-{scale}x.log").open("w") as log:
                process = sp.Popen([str(SMOKE.BINARY), str(note)], env=env, stdout=log, stderr=log)
                try:
                    window = SMOKE.run("xdotool", "search", "--sync", "--onlyvisible", "--pid", str(process.pid)).splitlines()[0]
                    assert Path(f"/proc/{process.pid}/exe").samefile(SMOKE.BINARY)
                    SMOKE.run("xdotool", "windowsize", "--sync", window, str(1100 * scale), str(800 * scale))
                    SMOKE.run("xdotool", "windowfocus", "--sync", window)

                    def state_matches(key, expected):
                        try:
                            value = json.loads(session.read_text())[key]
                            return value == expected if key != "theme" else value["name"] == expected
                        except (FileNotFoundError, json.JSONDecodeError, KeyError):
                            return False

                    def click(index):
                        SMOKE.run("xdotool", "mousemove", "--window", window,
                                  str((33 + 36 * index) * scale), str(76 * scale), "click", "1")
                        SMOKE.run("xdotool", "mousemove", "--window", window, str(1000 * scale), str(740 * scale))

                    def capture(name):
                        time.sleep(0.15)  # Allow the final frame after persisted state, not an app readiness guess.
                        path = OUTPUT / f"{name}-{scale}x.png"
                        SMOKE.run("import", "-window", window, str(path))
                        with Image.open(path) as image:
                            assert image.size == (1100 * scale, 800 * scale)
                        screenshots.append(str(path))

                    for index, mode in [(6, "Edit"), (7, "Preview"), (8, "Split"), (9, "Edit"), (8, "Split")]:
                        click(index)
                        SMOKE.wait_for(lambda: state_matches("mode", mode), f"toolbar click selects {mode} at {scale}x")
                    click(11)
                    SMOKE.wait_for(lambda: state_matches("theme", "Velocidark"), "dark theme click")
                    capture("dark")
                    click(10)
                    SMOKE.wait_for(lambda: state_matches("theme", "Velocilight"), "light theme click")
                    capture("light")
                    click(3)
                    assert note.read_text() == content, "Save changed untouched note content"
                    click(11)
                    SMOKE.wait_for(lambda: state_matches("theme", "Velocidark"), "dark theme restored")
                    if scale == 1:
                        SMOKE.run("xdotool", "mousemove", "--window", window, "285", "76")
                        time.sleep(0.8)
                        capture("hover-preview")
                    SMOKE.close_window(window)
                    assert process.wait(timeout=8) == 0
                    exits.append(process.returncode)
                    assert note.read_text() == content
                finally:
                    if process.poll() is None:
                        process.terminate()
                        process.wait(timeout=5)
    # Crop-only composition of real native screenshots, no redrawn UI.
    strips = Image.new("RGB", (1100, 216))
    for row, theme in enumerate(["dark", "light"]):
        with Image.open(OUTPUT / f"{theme}-1x.png") as image:
            strips.paste(image.crop((0, 0, 1100, 108)), (0, row * 108))
    strips.save(OUTPUT / "native-toolbar-comparison.png")
    sheet = source_sheet()
    with SMOKE.BINARY.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    result = {"passed": True, "binary": str(SMOKE.BINARY), "binary_sha256": digest,
              "checks": ["real toolbar mode clicks", "real theme clicks", "save preserves note bytes",
                         "normal and 2x native captures", "hover tooltip", "graceful exit"],
              "screenshots": screenshots, "source_sheet": sheet, "exit_codes": exits}
    result_file.write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Linux native QA for toolbar, saved Split resizing, and an opt-in real folder picker."""
import argparse
import datetime as dt
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import select
import signal
import subprocess as sp
import sys
import time
import traceback

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/velocimd')
    parser.add_argument('--native-picker', action='store_true', help='exercise an actual Linux folder picker in the private Xvfb display')
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = ROOT / 'target/desktop-controls/native' / dt.datetime.now(dt.timezone.utc).strftime('%Y%m%dT%H%M%S%fZ')
    output.mkdir(parents=True)
    sha = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
    result = {'binary': str(binary), 'binary_sha256': sha(binary), 'harness_sha256': sha(Path(__file__)),
              'checks': [], 'screenshots': [], 'failures': [], 'commands': []}
    note = output / 'Controls.md'
    body = ''.join(f'## SECTION {i:04}\n\n' + 'Original readable Markdown. ' * 12 + '\n\n' for i in range(200))
    note.write_text(body, encoding='utf-8')
    selected = output / 'Selected folder'
    selected.mkdir()
    session = output / 'config/velocimd/state.json'
    session.parent.mkdir(parents=True)
    session.write_text(json.dumps({'documents': [{'id': 1, 'title': note.name, 'path': str(note), 'dirty': False}],
                                  'active_document': 0, 'mode': 'Split', 'working_folder': str(output),
                                  'folder_tabs': [{'path': str(output)}], 'active_folder_tab': 0,
                                  'window_size': [1100.0, 800.0]}), encoding='utf-8')
    env = dict(os.environ, XDG_CONFIG_HOME=str(output / 'config'), XDG_DATA_HOME=str(output / 'data'),
               XDG_CACHE_HOME=str(output / 'cache'), LIBGL_ALWAYS_SOFTWARE='1', GALLIUM_DRIVER='llvmpipe',
               WINIT_UNIX_BACKEND='x11', WINIT_X11_SCALE_FACTOR='1', GDK_BACKEND='x11', GTK_USE_PORTAL='0',
               GSETTINGS_BACKEND='memory', NO_AT_BRIDGE='1', DBUS_SESSION_BUS_ADDRESS='unix:path=/nonexistent')
    env.pop('WAYLAND_DISPLAY', None)
    spec = importlib.util.spec_from_file_location('desktop_native_helpers', ROOT / 'scripts/native-smoke.py')
    assert spec is not None and spec.loader is not None
    helpers = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(helpers)
    apps = []
    app = xvfb = None
    window = None

    def command(argv, timeout=20, required=True):
        p = sp.run(list(map(str, argv)), env=env, cwd=ROOT, capture_output=True, text=True, timeout=timeout)
        result['commands'].append({'argv': list(map(str, argv)), 'exit_code': p.returncode, 'stdout': p.stdout, 'stderr': p.stderr})
        if required:
            assert p.returncode == 0, f'command failed: {argv}: {p.stderr}'
        return p.stdout.strip() if p.returncode == 0 else None

    def state():
        return json.loads(session.read_text(encoding='utf-8'))

    def wait(predicate, description, timeout=10):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            try:
                if predicate():
                    return
            except (OSError, ValueError):
                pass
            assert app is None or app.poll() is None, 'owned app exited unexpectedly'
            time.sleep(0.05)
        raise AssertionError(description)

    def capture(label):
        time.sleep(0.3)
        path = output / (label + '.png')
        command(['import', '-window', window, path])
        result['screenshots'].append({'path': str(path), 'sha256': sha(path)})

    def click(x, y, repeat=1):
        command(['xdotool', 'mousemove', '--window', window, x, y])
        time.sleep(0.1)
        command(['xdotool', 'click', '--repeat', repeat, '--delay', '100', '1'])

    def key(value):
        command(['xdotool', 'key', '--clearmodifiers', value])

    def divider_x(width, ratio):
        available = width - 28.0
        minimum = min(180.0 / available, 0.5)
        return round(14.0 + available * min(max(ratio, minimum), 1.0 - minimum))

    def drag(x, target):
        command(['xdotool', 'mousemove', '--window', window, x, 350])
        time.sleep(0.15)
        command(['xdotool', 'mousedown', '1'])
        time.sleep(0.1)
        command(['xdotool', 'mousemove', '--window', window, target, 350])
        time.sleep(0.2)
        command(['xdotool', 'mouseup', '1'])

    def start(label):
        nonlocal app, window
        with (output / (label + '.log')).open('w') as log:
            app = sp.Popen([str(binary)], env=env, cwd=ROOT, stdout=log, stderr=log, start_new_session=True)
        apps.append(app)
        windows = command(['xdotool', 'search', '--sync', '--onlyvisible', '--pid', app.pid], 30)
        assert windows
        window = windows.splitlines()[0]
        assert Path(f'/proc/{app.pid}/exe').samefile(binary)
        command(['xdotool', 'windowfocus', '--sync', window])
        command(['xdotool', 'windowsize', '--sync', window, 1100, 800])
        time.sleep(0.4)

    def close():
        assert app is not None
        old = os.environ.get('DISPLAY')
        os.environ['DISPLAY'] = env['DISPLAY']
        try:
            helpers.close_window(window)
        finally:
            if old is None:
                os.environ.pop('DISPLAY', None)
            else:
                os.environ['DISPLAY'] = old
        assert app.wait(timeout=20) == 0

    try:
        read_fd, write_fd = os.pipe()
        try:
            with (output / 'xvfb.log').open('w') as log:
                xvfb = sp.Popen(['Xvfb', '-displayfd', str(write_fd), '-screen', '0', '1280x1000x24', '-nolisten', 'tcp'],
                                pass_fds=(write_fd,), env=env, stdout=log, stderr=log)
        finally:
            os.close(write_fd)
        try:
            assert select.select([read_fd], [], [], 8)[0], 'private Xvfb not ready'
            display = ':' + os.read(read_fd, 64).decode().strip()
        finally:
            os.close(read_fd)
        assert display[1:].isdigit()
        env['DISPLAY'] = display
        result['display'] = display
        start('app-first')
        capture('initial-dark-split')
        for theme in ['Velocilight', 'Velocidark']:
            click(394, 75)
            wait(lambda: state().get('theme', {}).get('name') == theme, 'single theme button must toggle')
            capture('toggle-' + theme.lower())
            result['checks'].append({'check': 'single theme button', 'theme': theme, 'passed': True})
        for mode, x in [('Edit', 250), ('Split', 286), ('Preview', 322), ('Split', 286)]:
            click(x, 75)
            wait(lambda: state()['mode'] == mode, 'mode toolbar must follow Edit-Split-View order')
            result['checks'].append({'check': 'mode button', 'mode': mode, 'x': x, 'passed': True})
        drag(550, 800)
        wait(lambda: 0.70 < state().get('split_ratio', 0.5) < 0.76, 'divider must resize and save its position')
        saved = state()['split_ratio']
        capture('resized-split')
        assert note.read_text(encoding='utf-8') == body
        close()
        start('app-reopened')
        assert state()['split_ratio'] == saved
        capture('restored-split')
        result['checks'].append({'check': 'divider drag/save/reopen', 'split_ratio': saved, 'passed': True})
        click(divider_x(1100, saved), 350, repeat=2)
        wait(lambda: state()['split_ratio'] == 0.5, 'double-click must reset to balanced split')
        capture('reset-split')
        command(['xdotool', 'windowsize', '--sync', window, 560, 700])
        time.sleep(0.3)
        drag(280, 500)
        wait(lambda: 0.6 < state()['split_ratio'] < 0.7, 'narrow layout must bound divider and preserve both panes')
        capture('narrow-resized-split')
        result['checks'].append({'check': 'divider reset and narrow bounds', 'split_ratio': state()['split_ratio'], 'passed': True})
        command(['xdotool', 'windowsize', '--sync', window, 1100, 800])
        time.sleep(0.3)
        if args.native_picker:
            # The folder-tab + is distinct from the new-file toolbar +.
            # Coordinates are bound to the captured fixed-width fixture title,
            # 1100x800 viewport and scale=1 environment above.
            click(310, 31)
            dialogs = command(['xdotool', 'search', '--sync', '--onlyvisible', '--name', '^Open Folder Tab$'], 15)
            assert dialogs
            dialog = dialogs.splitlines()[0]
            assert dialog != window
            command(['xdotool', 'windowfocus', '--sync', dialog])
            # GTK maps the window before initial folder loading/focus completes.
            time.sleep(1.0)
            picker = output / 'owned-folder-picker.png'
            command(['import', '-window', dialog, picker])
            result['screenshots'].append({'path': str(picker), 'sha256': sha(picker)})
            key('ctrl+l')
            time.sleep(0.2)
            key('ctrl+a')
            command(['xdotool', 'type', '--clearmodifiers', '--delay', '0', str(selected) + os.sep])
            time.sleep(0.2)
            entered = output / 'picker-entered-path.png'
            command(['import', '-window', dialog, entered])
            result['screenshots'].append({'path': str(entered), 'sha256': sha(entered)})
            key('Return')
            time.sleep(0.25)
            if command(['xdotool', 'getwindowname', dialog], required=False) is not None:
                key('alt+o')
            def selected_folder_tab():
                current = state()
                return Path(current['folder_tabs'][current['active_folder_tab']]['path']).resolve() == selected.resolve()
            wait(selected_folder_tab, 'actual picker must open the exact selected folder tab')
            assert Path(state()['working_folder']).resolve() == output.resolve(), 'opening a tab must preserve the default working folder'
            result['checks'].append({'check': 'actual new-folder-tab button and Linux native picker', 'selected': str(selected), 'passed': True})
            capture('after-native-folder-selection')
        assert note.read_text(encoding='utf-8') == body, 'controls must never modify the note'
        close()
        assert note.read_text(encoding='utf-8') == body
        result['note_unchanged'] = True
    except Exception as exc:
        result['failures'].append({'error': str(exc), 'traceback': traceback.format_exc()})
        if window is not None and app is not None and app.poll() is None:
            try:
                capture('failure')
            except Exception:
                pass
    finally:
        for process in apps:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGTERM)
                try:
                    process.wait(timeout=10)
                except sp.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGKILL)
                    process.wait(timeout=10)
        if xvfb is not None and xvfb.poll() is None:
            xvfb.terminate()
            xvfb.wait(timeout=10)
        owned = apps + ([xvfb] if xvfb is not None else [])
        result['cleanup'] = {'pids': [p.pid for p in owned], 'exit_codes': [p.returncode for p in owned],
                             'processes_absent': all(not Path(f'/proc/{p.pid}').exists() for p in owned)}
        result['passed'] = not result['failures'] and result['cleanup']['processes_absent']
        (output / 'result.json').write_text(json.dumps(result, indent=2) + '\n', encoding='utf-8')
        print(json.dumps({'evidence': str(output), 'passed': result['passed'], 'checks': result['checks'],
                          'failures': result['failures'], 'cleanup': result['cleanup']}, indent=2))
    return 0 if result['passed'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

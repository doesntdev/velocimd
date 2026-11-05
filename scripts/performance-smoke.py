#!/usr/bin/env python3
"""Linux-only isolated large-session native verification, exact current binary.

python3 scripts/performance-smoke.py [--binary target/debug/velocimd]
Uses owned Xvfb, llvmpipe, no clipboard or desktop input. Legacy 55x1MiB session
is seeded before launch only; inactive switching uses the actual folder menu.
Screenshots, exact initial/final disk hashes, commands, failures and process cleanup
are retained below target/performance-execution/native. No production edits.
"""
import argparse
import csv
import io
import datetime as dt
import hashlib
import importlib.util
import json
import re
import os
from pathlib import Path
import select
import subprocess as sp
import sys
import time
import traceback
from PIL import Image

sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]


def sha(path):
    with Path(path).open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2)+'\n')


def load_script(name):
    spec = importlib.util.spec_from_file_location(name.replace('-','_'), ROOT / 'scripts' / (name+'.py'))
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=ROOT / 'target/debug/velocimd')
    parser.add_argument('--reuse-smoke-receipts', type=Path, help='Prior owned run with passing normal/icon checks for this exact binary; avoids replaying completed smoke checks')
    args = parser.parse_args()
    previous = args.reuse_smoke_receipts.resolve() if args.reuse_smoke_receipts else None
    icon_result = None
    binary = args.binary.resolve()
    assert binary.is_file()
    root = ROOT / 'target/performance-execution/native' / dt.datetime.now(dt.timezone.utc).strftime('run-%Y%m%dT%H%M%S%fZ')
    root.mkdir(parents=True)
    os.environ['XDG_CONFIG_HOME'] = str(root / 'config')
    os.environ['VELOCIMD_SMOKE_BINARY'] = str(binary)
    os.environ['VELOCIMD_SMOKE_XVFB'] = '1'
    os.environ['VELOCIMD_ICON_XVFB'] = '1'
    for env_key,value in {'LIBGL_ALWAYS_SOFTWARE':'1','GALLIUM_DRIVER':'llvmpipe','WINIT_UNIX_BACKEND':'x11','WINIT_X11_SCALE_FACTOR':'1','NO_AT_BRIDGE':'1','DBUS_SESSION_BUS_ADDRESS':'unix:path=/nonexistent'}.items():
        os.environ[env_key] = value
    os.environ.pop('WAYLAND_DISPLAY',None)
    env = dict(os.environ)
    (root / 'charter.md').write_text('Local Linux only. Existing normal native smoke and 1x/2x toolbar smoke; actual 55x1MiB restore, End/Home scrolling, distant Split preview synchronization, exact autosave, actual inactive activation via public folder menu, graceful close/reopen, 560x700 and 1100x800, dark/light screenshots. Settings and notes exclusively owned; all xdotool input on owned Xvfb. Stop at first failed product contract; retain repro and cleanup. Every subprocess and readiness wait bounded. OCR is supporting evidence, screenshots require inspection.\n')
    result = {'binary':str(binary),'binary_sha256':sha(binary),'harness_sha256':sha(__file__),'checks':[], 'screenshots':[], 'commands':[], 'failures':[], 'platform':'local Linux/X11/llvmpipe only'}
    (root / 'performance-smoke.py').write_bytes(Path(__file__).read_bytes())
    result['runner_source_hashes'] = {name:sha(ROOT / 'scripts' / name) for name in ['native-smoke.py','icon-smoke.py']}
    smoke = load_script('native-smoke')
    processes = []
    xvfb = None
    stage = 'setup'

    def command(argv, timeout=30, check=True):
        proc = sp.run(list(map(str,argv)), env=env, cwd=ROOT, text=True, capture_output=True, timeout=timeout)
        entry = {'command':list(map(str,argv)), 'exit_code':proc.returncode,'stdout':proc.stdout,'stderr':proc.stderr}
        result['commands'].append(entry)
        save(root / 'progress.json',result)
        if check and proc.returncode:
            raise AssertionError(f'command failed: {argv}: {proc.stderr}')
        return proc

    def key(*keys):
        command(['xdotool','key','--clearmodifiers','--delay','100',*keys])

    def click(window,x,y):
        command(['xdotool','mousemove','--window',window,x,y,'click','1'])

    def capture(window,name):
        # Bounded final-frame settle after state/readiness, never background polling.
        time.sleep(.3)
        path = root / (name+'.png')
        command(['import','-window',window,path])
        with Image.open(path) as im:
            assert im.width >= 500 and im.height >= 600
        result['screenshots'].append({'path':str(path),'sha256':sha(path)})
        return path

    def ocr(path,box=None):
        source = path
        if box:
            source = root / (path.stem+'-pane-'+'-'.join(map(str,box))+'.png')
            with Image.open(path) as im:
                im.crop(box).resize(((box[2]-box[0])*2,(box[3]-box[1])*2)).save(source)
        out = command(['tesseract',source,'stdout','--psm','6']).stdout
        (root / (source.stem+'.txt')).write_text(out)
        return out.upper()

    def assert_split_capture(image):
        with Image.open(image) as im:
            width, height = im.size
        warning = ocr(image,(0,0,width,45))
        assert 'PERF WARNING' not in warning and 'REQUEST_DISCARD' not in warning, 'Split geometry/theme transition entered a repeated discard storm'
        left = re.findall(r'SECTION\s+([0-9]{4})', ocr(image,(0,90,width//2-5,height)))
        right = re.findall(r'SECTION\s+([0-9]{4})', ocr(image,(width//2+5,90,width,height)))
        assert left and right, 'both Split source contexts must be OCR-verifiable; inspect capture on failure'
        # At a real bottom clamp the denser pane can expose one earlier heading.
        # A stale target many sections away is never accepted as functional sync.
        assert abs(int(left[0])-int(right[0])) <= 1, f'Split source drift after geometry/theme transition: editor {left[0]}, preview {right[0]}'

    def checkpoint(name, data=None):
        result['checks'].append({'check':name,'passed':True,'details':data})
        save(root / 'progress.json',result)

    def start():
        log = (root / f'app-{len(processes)+1}.log').open('w')
        process = sp.Popen([str(binary)],env=env,stdout=log,stderr=log)
        processes.append(process)
        log.close()
        window = command(['xdotool','search','--sync','--onlyvisible','--pid',str(process.pid)],timeout=40).stdout.splitlines()[0]
        assert Path(f'/proc/{process.pid}/exe').samefile(binary)
        command(['xdotool','windowsize','--sync',window,'1100','800'])
        command(['xdotool','windowfocus','--sync',window])
        return process,window

    def state_matches(key,value):
        try:
            return json.loads(session.read_text())[key] == value
        except (FileNotFoundError,json.JSONDecodeError,KeyError):
            return False

    try:
        r,w = os.pipe()
        log = (root / 'xvfb.log').open('w')
        try:
            xvfb = sp.Popen(['Xvfb','-displayfd',str(w),'-screen','0','2560x1800x24','-nolisten','tcp'],pass_fds=(w,),env=env,stdout=log,stderr=log)
        finally:
            os.close(w)
            log.close()
        try:
            assert select.select([r],[],[],8)[0], 'Xvfb display readiness timeout'
            display = ':'+os.read(r,64).decode().strip()
        finally:
            os.close(r)
        assert display.startswith(':') and display[1:].isdigit()
        os.environ['DISPLAY'] = display
        env['DISPLAY'] = display
        result['display'] = display
        result['xvfb_pid'] = xvfb.pid
        stage = 'normal-smoke'
        if previous is not None:
            assert previous.is_relative_to(ROOT / 'target/performance-execution/native')
            normal_result = json.loads((previous / 'normal-smoke-result.json').read_text())
            assert normal_result['passed'] and normal_result['binary_sha256'] == result['binary_sha256']
            result['prior_smoke_receipts'] = str(previous)
        else:
            normal = command([sys.executable,ROOT / 'scripts/native-smoke.py'],timeout=180,check=False)
            save(root / 'normal-smoke-receipt.json',result['commands'][-1])
            assert normal.returncode == 0, 'existing normal native smoke failed'
            normal_result = json.loads((ROOT / 'target/native-smoke/result.json').read_text())
            assert normal_result['binary_sha256'] == result['binary_sha256']
        save(root / 'normal-smoke-result.json',normal_result)
        checkpoint('existing normal native smoke',normal_result)
        stage = 'toolbar-smoke'
        if previous is not None:
            icon_result = json.loads((previous / 'icon-result.json').read_text())
            assert icon_result['passed'] and icon_result['binary_sha256'] == result['binary_sha256']
            icon = None
        else:
            icon = command([sys.executable,ROOT / 'scripts/icon-smoke.py'],timeout=180,check=False)
            save(root / 'icon-original-receipt.json',result['commands'][-1])
        if icon is not None and icon.returncode:
            # Only the external SVG source-art renderer may be substituted. UI is not modified.
            assert 'source_sheet' in icon.stderr and 'convert' in icon.stderr, 'toolbar runtime failed, not source-sheet delegate'
            wrapper = root / 'icon-msvg-wrapper.py'
            wrapper.write_text('import importlib.util,sys\nsys.dont_write_bytecode=True\ns=importlib.util.spec_from_file_location("icons",'+repr(str(ROOT / 'scripts/icon-smoke.py'))+')\nm=importlib.util.module_from_spec(s);s.loader.exec_module(m)\noriginal=m.SMOKE.run\ndef run(*args,**kw):\n    if args and args[0]=="convert":\n        args=tuple("msvg:"+a if isinstance(a,str) and a.endswith(".svg") else a for a in args)\n    return original(*args,**kw)\nm.SMOKE.run=run\nraise SystemExit(m.main())\n')
            fallback = command([sys.executable,wrapper],timeout=180,check=False)
            save(root / 'icon-msvg-receipt.json',result['commands'][-1])
            assert fallback.returncode == 0, 'MSVG source-sheet fallback failed'
            result['source_sheet_delegate_failure_preserved'] = True
        if previous is None:
            icon_result = json.loads((ROOT / 'target/icon-review/result.json').read_text())
        assert icon_result is not None
        assert icon_result['binary_sha256'] == result['binary_sha256']
        save(root / 'icon-result.json',icon_result)
        result['existing_smoke_artifact_hashes'] = {p:sha(p) for p in normal_result['screenshots']+icon_result['screenshots']+[icon_result['source_sheet']]}
        checkpoint('normal and 2x native toolbar smoke',icon_result)
        stage = 'large-session-fixture'
        notes = root / 'Notes'
        notes.mkdir()
        docs, initial_hashes = [], {}
        originals = {}
        for i in range(55):
            begin = f'# NOTE {i:03d} BEGIN\n\n'
            block = ''.join(f'## NOTE {i:03d} SECTION {j:04d}\n\n'+('Clear ideas in a distant paragraph. '*14)+'\n\n' for j in range(2200))
            end = f'\n\n## NOTE {i:03d} END'
            content = begin+block[:1024*1024-len(begin)-len(end)]+end
            assert len(content.encode()) == 1024*1024
            path = notes / f'Note-{i:03d}.md'
            path.write_text(content)
            originals[i] = content
            initial_hashes[path.name] = sha(path)
            docs.append({'id':i+1,'title':path.name,'path':str(path),'dirty':False})
        session = root / 'config/velocimd/state.json'
        initial_state = {'documents':docs,'active_document':0,'mode':'Edit','folder_tabs':[{'path':str(notes)}],'active_folder_tab':0,'working_folder':str(notes),'window_size':[1100.,800.]}
        save(session,initial_state)
        save(root / 'initial-state.json',initial_state)
        save(root / 'initial-note-hashes.json',initial_hashes)
        stage = 'large-session-startup'
        process,window = start()
        image = capture(window,'large-edit-home-dark')
        assert 'BEGIN' in ocr(image), 'initial active note did not visibly render'
        checkpoint('55-note startup visibly renders active 1MiB note')
        click(window,250,240)
        stage = 'End-Home-editor-scroll'
        key('ctrl+End')
        image = capture(window,'large-edit-end-dark')
        assert 'END' in ocr(image,(0,90,1100,800)), 'Ctrl+End failed to reveal end marker'
        key('ctrl+Home')
        image = capture(window,'large-edit-return-home-dark')
        assert 'BEGIN' in ocr(image,(0,90,1100,800)), 'Ctrl+Home failed to reveal begin marker'
        checkpoint('real editor Ctrl+End and Ctrl+Home scroll')
        stage = 'split-distant-sync'
        key('ctrl+3')
        smoke.wait_for(lambda:state_matches('mode','Split'),'Split persists',seconds=30)
        click(window,250,240)
        key('ctrl+End')
        image = capture(window,'large-split-end-dark')
        warning = ocr(image,(0,0,1100,45))
        assert 'PERF WARNING' not in warning and 'REQUEST_DISCARD' not in warning, 'Native Split entered a repeated egui discard storm (visible performance warning)'
        left = ocr(image,(0,90,545,800))
        assert 'END' in left, 'Split editor end marker missing'
        # Scroll follows the top source line, not the caret. Click the visible END
        # source line to demand an exact far-source anchor before checking preview.
        click(window,210,776)
        image = capture(window,'large-split-end-click-sync')
        right = ocr(image,(555,90,1100,800))
        assert 'END' in right, 'Distant Split preview did not synchronize to clicked end marker'
        key('ctrl+Home')
        image = capture(window,'large-split-home-dark')
        assert 'BEGIN' in ocr(image,(555,90,1100,800)), 'Split preview did not return to beginning'
        checkpoint('Split preview synchronizes distant end and beginning')
        stage = 'exact-large-note-autosave'
        key('ctrl+1')
        smoke.wait_for(lambda:state_matches('mode','Edit'),'Edit persists',seconds=30)
        click(window,250,240)
        key('ctrl+End','Return')
        marker = 'Native large-session exact autosave verified.'
        command(['xdotool','type','--clearmodifiers','--delay','30',marker])
        expected = originals[0]+'\n'+marker
        smoke.wait_for(lambda:(notes / 'Note-000.md').read_text()==expected,'large note exact autosave',seconds=30)
        capture(window,'large-edit-autosaved')
        checkpoint('exact 1MiB note autosave',{'expected_bytes':len(expected),'sha256':sha(notes / 'Note-000.md')})
        stage = 'inactive-note-public-folder-menu'
        click(window,130,30)
        image = capture(window,'folder-menu-before-activation')
        # The owned evidence path can wrap the folder header, moving every
        # row. Locate the actual public Note-001 label instead of clicking a
        # hard-coded y (which silently reopened Note-000 at this path length).
        text = ocr(image)
        assert 'NOTE-001' in text or 'NOTE-O01' in text or 'NOTE-0O1' in text, 'inactive note not visible in public folder menu'
        tsv = command(['tesseract',image,'stdout','--psm','6','tsv']).stdout
        (root / 'folder-menu-before-activation.tsv').write_text(tsv)
        rows = [row for row in csv.DictReader(io.StringIO(tsv), delimiter='\t') if row['text'].upper().strip() == 'NOTE-001']
        assert len(rows) == 1, 'public menu Note-001 label must have one unambiguous OCR bounding box'
        row = rows[0]
        target_x = int(row['left']) + int(row['width']) // 2
        target_y = int(row['top']) + int(row['height']) // 2
        result['inactive_menu_target'] = {'label':'Note-001','x':target_x,'y':target_y,'ocr_box':row}
        click(window,target_x,target_y)
        smoke.wait_for(lambda:state_matches('active_document',1),'public menu activates inactive note',seconds=30)
        image = capture(window,'inactive-note-001-activated')
        assert 'BEGIN' in ocr(image), 'activated note body blank'
        click(window,250,240)
        key('ctrl+End','Return')
        second_marker = 'Previously inactive note hydration verified.'
        command(['xdotool','type','--clearmodifiers','--delay','30',second_marker])
        second_expected = originals[1]+'\n'+second_marker
        smoke.wait_for(lambda:(notes / 'Note-001.md').read_text()==second_expected,'inactive note exact autosave after hydration',seconds=30)
        checkpoint('actual public folder-menu inactive activation and exact persisted body')
        stage = 'narrow-normal-light-dark'
        key('ctrl+3','alt+l')
        smoke.wait_for(lambda:state_matches('mode','Split') and json.loads(session.read_text())['theme']['name']=='Velocilight','light Split persists',seconds=30)
        assert_split_capture(capture(window,'large-split-normal-light'))
        command(['xdotool','windowsize','--sync',window,'560','700'])
        assert_split_capture(capture(window,'large-split-narrow-light'))
        key('alt+d')
        smoke.wait_for(lambda:json.loads(session.read_text())['theme']['name']=='Velocidark','dark theme persists',seconds=30)
        assert_split_capture(capture(window,'large-split-narrow-dark'))
        command(['xdotool','windowsize','--sync',window,'1100','800'])
        assert_split_capture(capture(window,'large-split-normal-dark'))
        checkpoint('narrow and normal windows, light and dark themes; captures require visual inspection')
        stage = 'graceful-close-reopen'
        smoke.close_window(window)
        assert process.wait(timeout=30) == 0
        final = json.loads(session.read_text())
        assert final['active_document'] == 1 and len(final['documents']) == 55
        save(root / 'closed-state.json',final)
        process,window = start()
        key('ctrl+1')
        smoke.wait_for(lambda:state_matches('mode','Edit'),'reopened editor',seconds=30)
        click(window,250,240)
        key('ctrl+End')
        image = capture(window,'large-reopened-active-end')
        assert 'HYDRATION' in ocr(image), 'reopened active body missing exact saved marker'
        assert (notes / 'Note-000.md').read_text()==expected
        assert (notes / 'Note-001.md').read_text()==second_expected
        smoke.close_window(window)
        assert process.wait(timeout=30) == 0
        for i in range(2,55):
            assert sha(notes / f'Note-{i:03d}.md')==initial_hashes[f'Note-{i:03d}.md'], 'untouched note changed'
        save(root / 'final-note-hashes.json',{p.name:sha(p) for p in sorted(notes.glob('*.md'))})
        save(root / 'reopened-final-state.json',json.loads(session.read_text()))
        checkpoint('graceful close/reopen exact persisted notes and 53 untouched bodies')
    except Exception as exc:
        result['failures'].append({'stage':stage,'error':str(exc),'traceback':traceback.format_exc()})
        for p in processes:
            if p.poll() is None:
                try:
                    windows = command(['xdotool','search','--onlyvisible','--pid',str(p.pid)],check=False).stdout.splitlines()
                    if windows:
                        capture(windows[0],'failure-'+stage)
                except Exception:
                    pass
    finally:
        for p in processes:
            if p.poll() is None:
                p.terminate()
                try:
                    p.wait(timeout=10)
                except sp.TimeoutExpired:
                    p.kill(); p.wait(timeout=10)
        if xvfb and xvfb.poll() is None:
            xvfb.terminate(); xvfb.wait(timeout=10)
        tracked = [p.pid for p in processes]+([xvfb.pid] if xvfb else [])
        result['cleanup'] = {'owned_pids':tracked,'exit_codes':[p.returncode for p in processes], 'xvfb_exit':xvfb.returncode if xvfb else None,'processes_absent':all(not Path(f'/proc/{pid}').exists() for pid in tracked)}
        # Read-only check for surviving applications bound to our private display.
        survivors = []
        for p in Path('/proc').iterdir():
            if not p.name.isdigit() or int(p.name)==os.getpid():
                continue
            try:
                e = (p/'environ').read_bytes().split(b'\0')
                if ('DISPLAY='+env.get('DISPLAY','never')).encode() in e and (p/'exe').resolve()==binary:
                    survivors.append(int(p.name))
            except (FileNotFoundError,PermissionError,ProcessLookupError):
                pass
        result['cleanup']['display_app_survivors'] = survivors
        result['binary_unchanged'] = sha(binary)==result['binary_sha256']
        result['passed'] = not result['failures'] and result['cleanup']['processes_absent'] and not survivors and result['binary_unchanged']
        save(root / 'result.json',result)
        print(json.dumps({'evidence':str(root),'passed':result['passed'],'checks':result['checks'],'failures':result['failures'],'cleanup':result['cleanup'],'binary_sha256':result['binary_sha256']},indent=2))
    return 0 if result['passed'] else 1


if __name__=='__main__':
    raise SystemExit(main())

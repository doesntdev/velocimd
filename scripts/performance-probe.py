#!/usr/bin/env python3
"""Reproducible paired CPU probe. No production edits; one shared Cargo target cache.

python3 scripts/performance-probe.py [--rounds 4]
Snapshots immutable base and frozen working sources; builds release dependencies once
with the actual compiler, then rustc-compiles both arms against identical artifacts.
All evidence, raw samples, commands and hashes stay in target/performance-execution/bench.
"""
import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import shutil
import statistics as st
import subprocess as sp
import tomllib

ROOT = Path(__file__).resolve().parents[1]
BASE = "61243e987efbdc7f3128fc73fabc462cd3dd4a13"


def sha(path):
    with Path(path).open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def save(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rounds', type=int, default=4)
    parser.add_argument('--preview-only', action='store_true', help='Rerun affected preview arms only, preserving all original targets')
    args = parser.parse_args()
    assert args.rounds >= 2
    root = ROOT / 'target/performance-execution/bench' / dt.datetime.now(dt.timezone.utc).strftime('run-%Y%m%dT%H%M%S%fZ')
    root.mkdir(parents=True)
    env = dict(os.environ, XDG_CONFIG_HOME=str(root / 'config'))
    (root / 'charter.md').write_text('Paired CPU-only Context::run_ui: Edit 100KiB short lines >=70% reduction; Split plain 1MiB <8ms warm; Preview rich 100KiB measured; 10k-entry menu <1ms warm with separate cache/scan; 55x1MiB restore only one initial hydrated body. No GPU or native latency claim. Fixed sources, common dependencies, alternating process order, raw samples retained. All commands have finite timeouts.\n')
    receipts = []

    def command(argv, label, timeout=600, extra=None):
        proc = sp.run(argv, cwd=ROOT, env=env if extra is None else dict(env, **extra), text=True, capture_output=True, timeout=timeout)
        receipt = {'command': list(map(str, argv)), 'cwd': str(ROOT), 'XDG_CONFIG_HOME': env['XDG_CONFIG_HOME'], 'exit_code': proc.returncode, 'stdout': proc.stdout, 'stderr': proc.stderr}
        save(root / f'{label}.json', receipt)
        receipts.append(str(root / f'{label}.json'))
        if proc.returncode:
            raise RuntimeError(f'{label} failed; see {root / (label + ".json")}')
        return proc.stdout

    source_files = sorted(p for p in (ROOT / 'src').rglob('*') if p.is_file())
    source_files += sorted(p for p in (ROOT / 'assets').rglob('*') if p.is_file())
    source_files += [ROOT / p for p in ['Cargo.toml', 'Cargo.lock', 'build.rs']]
    initial = {str(p.relative_to(ROOT)): sha(p) for p in source_files}
    save(root / 'candidate-original-hashes.json', initial)
    command(['git', 'status', '--porcelain=v2', '--branch'], 'initial-git-state')
    compiler = command(['rustc', '-vV'], 'compiler')
    command(['cargo', '--version'], 'cargo-version')
    command(['uname', '-a'], 'host')
    command(['lscpu'], 'cpu')
    command(['git', 'diff', '--binary'], 'candidate-diff')
    # Immutable local snapshots; never copy target/dependencies.
    baseline_files = command(['git', 'ls-tree', '-r', '--name-only', BASE, '--', 'src', 'assets', 'Cargo.toml', 'Cargo.lock', 'build.rs'], 'base-file-list').splitlines()
    for rel in baseline_files:
        dest = root / 'baseline' / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        dest.write_bytes(sp.check_output(['git', 'show', f'{BASE}:{rel}'], cwd=ROOT))
    for path in source_files:
        dest = root / 'candidate' / path.relative_to(ROOT)
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(path, dest)
    for filename in ['performance-probe.rs', 'performance-probe.py']:
        shutil.copyfile(ROOT / 'scripts' / filename, root / filename)
    # Identical deterministic fixtures, shared by both arms.
    fixtures = root / 'fixtures'
    fixtures.mkdir()
    short = ''.join(f'line {i:05d}\n' for i in range(10240))  # exactly 100KiB, 10240 lines
    plain = ''
    i = 0
    while len(plain) < 1024*1024:
        plain += f'A calm line of plain text {i} for realistic editing and wrapping. More words here.\n'
        i += 1
    plain = plain[:1024*1024]
    rich = ''
    i = 0
    while len(rich) < 100*1024:
        rich += f'## Section {i}\n\nA paragraph with **strong text**, *emphasis* and a [link](https://example.invalid).\n\n- First item {i}\n- Second item\n\n'
        i += 1
    for name, content in [('short-lines-100KiB',short), ('plain-1MiB',plain), ('rich-100KiB',rich)]:
        (fixtures / f'{name}.md').write_text(content)
    folder = fixtures / 'folder-10000'
    folder.mkdir()
    for i in range(10000):
        (folder / f'Note-{(i*739)%10000:05d}.md').write_text('')
    notes = fixtures / 'notes-55'
    notes.mkdir()
    docs = []
    for i in range(55):
        path = notes / f'Note-{i:03d}.md'
        path.write_text(plain)
        docs.append({'id':i+1, 'title':path.name, 'path':str(path), 'dirty':False})
    save(fixtures / 'session-55.json', {'documents':docs, 'active_document':0, 'working_folder':str(notes), 'folder_tabs':[{'path':str(notes)}]})
    save(root / 'fixture-hashes.json', {str(p.relative_to(fixtures)):sha(p) for p in sorted(fixtures.rglob('*')) if p.is_file()})
    build = command(['cargo', 'build', '--release', '--locked', '--lib', '--message-format=json'], 'release-dependency-build', timeout=600)
    artifacts = [json.loads(line) for line in build.splitlines() if line.startswith('{')]
    libs = {}
    used = {}
    for a in artifacts:
        if a.get('reason') != 'compiler-artifact':
            continue
        for f in a.get('filenames', []):
            if f.endswith(('.rlib', '.so')) and Path(f).exists():
                used[f] = sha(f)
                if f.endswith('.rlib'):
                    libs[a['target']['name']] = f
    save(root / 'dependency-artifact-hashes.json', used)
    manifest = tomllib.loads((ROOT / 'Cargo.toml').read_text())
    direct = set(manifest['dependencies']) | {'native-dialog'}
    extern = []
    for name in sorted(direct):
        name = name.replace('-', '_')
        extern += ['--extern', f'{name}={libs[name]}']
    binaries = {}
    for arm in ['baseline', 'candidate']:
        snap = root / arm
        save(root / f'{arm}-original-snapshot-hashes.json',{str(p.relative_to(snap)):sha(p) for p in sorted(snap.rglob('*')) if p.is_file()})
        ui_path = snap / 'src/ui.rs'
        original_ui = ui_path.read_bytes()
        append_delta = '\n#[path=' + json.dumps(str(root / 'performance-probe.rs')) + ']\npub mod performance_probe;\n'
        with ui_path.open('a') as f:
            f.write(append_delta)
        assert ui_path.read_bytes() == original_ui + append_delta.encode()
        save(root / f'{arm}-authorized-harness-delta.json',{'target':'src/ui.rs','original_sha256':hashlib.sha256(original_ui).hexdigest(),'append_utf8':append_delta,'compiled_sha256':sha(ui_path),'only_append':True})
        preview_path = snap / 'src/ui/preview.rs'
        if preview_path.exists():
            preview_source = preview_path.read_text()
            measured = preview_source.replace('fn update(&mut self, markdown: &str, reuse: bool) {', 'fn update(&mut self, markdown: &str, reuse: bool) {\nlet perf_started = std::time::Instant::now();', 1)
            end_update = 'self.heights = Heights::from_blocks(&self.blocks);\n    }'
            assert measured.count(end_update) == 1
            measured = measured.replace(end_update, 'self.heights = Heights::from_blocks(&self.blocks);\nsuper::performance_probe::record_component("preview-update", perf_started.elapsed().as_secs_f64()*1000.);\n    }', 1)
            block_start = 'let block = &mut self.blocks[index];'
            assert measured.count(block_start) >= 1
            # Use the render_block occurrence, not trim_retained.
            render_at = measured.index('    fn render_block(')
            prefix, body = measured[:render_at], measured[render_at:]
            body = body.replace(block_start, 'let perf_started = std::time::Instant::now();\n'+block_start, 1)
            body = body.replace('        height\n    }', '        super::performance_probe::record_component("preview-block-layout-render", perf_started.elapsed().as_secs_f64()*1000.);\n        height\n    }', 1)
            measured = prefix + body
            preview_path.write_text(measured)
            save(root / f'{arm}-preview-timing-delta.json', {'original_sha256':hashlib.sha256(preview_source.encode()).hexdigest(),'instrumented_sha256':sha(preview_path),'purpose':'Snapshot-only component timers; no algorithm changes','original':preview_source,'instrumented':measured})
        # Ordinary crate layout is essential: path attributes on ui.rs change nested
        # module resolution and can accidentally bind ui::persistence to the model.
        declarations = [line for line in (snap / 'src/lib.rs').read_text().splitlines() if line.startswith(('pub mod ', 'mod '))]
        main = snap / 'src/probe-main.rs'
        main.write_text('\n'.join(declarations) + '\nfn main(){ui::performance_probe::run();}\n')
        binary = root / f'probe-{arm}'
        argv = ['rustc','--edition=2024','--crate-name',f'velocimd_perf_{arm}','-C','opt-level=3','-C','debuginfo=1','-L',f'dependency={ROOT / "target/release/deps"}',str(main),'-o',str(binary),*extern]
        command(argv, f'compile-{arm}')
        binaries[arm] = {'path':str(binary),'sha256':sha(binary)}
        save(root / f'{arm}-snapshot-hashes.json',{str(p.relative_to(snap)):sha(p) for p in sorted(snap.rglob('*')) if p.is_file()})
    outputs = {a:[] for a in binaries}
    order = []
    for round_no in range(args.rounds):
        for arm in (['baseline','candidate'] if round_no % 2 == 0 else ['candidate','baseline']):
            label = f'round-{round_no+1:02d}-{arm}'
            order.append(label)
            save(root / 'run-order.json',order)
            stdout = command([binaries[arm]['path']], label, timeout=240, extra={'PERF_ROOT':str(root / label), 'PERF_FIXTURES':str(fixtures),'PERF_ARM':arm, **({'PERF_PREVIEW_ONLY':'1'} if args.preview_only else {})})
            result = json.loads(stdout)
            save(root / (label+'-samples.json'),result)
            outputs[arm].append(result)
    summary = {'base':BASE,'compiler':compiler,'binaries':binaries,'run_order':order,'measurement':'CPU-only; no GPU or native end-to-end timing','rounds':args.rounds,'results':{}}
    for arm, rows in outputs.items():
        result = {}
        for index,name in enumerate(['plain-split','rich-preview'] if args.preview_only else ['short-lines-edit','plain-split','rich-preview']):
            vals = [v for row in rows for v in row['ui'][index]['warm_ms']]
            edits = [v for row in rows for v in row['ui'][index]['revision_ms']]
            result[name] = {'warm_median_ms':st.median(vals),'warm_min_ms':min(vals),'warm_max_ms':max(vals),'revision_median_ms':st.median(edits),'samples':len(vals),'round_medians_ms':[st.median(row['ui'][index]['warm_ms']) for row in rows]}
        if args.preview_only:
            summary['results'][arm] = result
            continue
        for name in ['scan_sort_ms','cache_hit_ms','menu_warm_ms']:
            vals = [v for row in rows for v in row['folder'][name]]
            result[name] = {'median_ms':st.median(vals),'samples':len(vals)}
        vals = [v for row in rows for v in row['restore']['warm_ms']]
        result['restore'] = {'warm_median_ms':st.median(vals),'observations':[row['restore'] for row in rows]}
        summary['results'][arm] = result
    b,c = summary['results']['baseline'],summary['results']['candidate']
    reduction = 100*(1-c['short-lines-edit']['warm_median_ms']/b['short-lines-edit']['warm_median_ms']) if not args.preview_only else None
    summary['acceptance'] = {'split_warm_ms':c['plain-split']['warm_median_ms'],'R2_pass':c['plain-split']['warm_median_ms']<8,
        'revision_ratio':c['plain-split']['revision_median_ms']/b['plain-split']['revision_median_ms'],
        'revision_pass':c['plain-split']['revision_median_ms'] <= b['plain-split']['revision_median_ms']*1.1}
    if not args.preview_only:
        assert reduction is not None
        summary['acceptance'].update({'short_lines_reduction_percent':reduction,'R1_pass':reduction>=70,'menu_warm_ms':c['menu_warm_ms']['median_ms'],'D1_pass':c['menu_warm_ms']['median_ms']<1,'L1_pass':all(row['restore']['hydrated_nonempty_buffers']==1 and row['restore']['resident_content_bytes']==1024*1024 for row in outputs['candidate'])})
    summary['source_unchanged_during_execution'] = initial == {str(p.relative_to(ROOT)):sha(p) for p in source_files}
    save(root / 'summary.json',summary)
    save(root / 'receipts.json',receipts)
    print(json.dumps({'evidence':str(root),'acceptance':summary['acceptance'],'binaries':binaries,'source_unchanged':summary['source_unchanged_during_execution']},indent=2))
    return 0 if all(summary['acceptance'][k] for k in (['R2_pass','revision_pass'] if args.preview_only else ['R1_pass','R2_pass','D1_pass','L1_pass','revision_pass'])) and summary['source_unchanged_during_execution'] else 1


if __name__ == '__main__':
    raise SystemExit(main())

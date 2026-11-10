#!/usr/bin/env python3
"""Capture effective qualified build settings at the actual producer boundary."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tomllib

p = argparse.ArgumentParser(description=__doc__)
p.add_argument('cargo', type=Path)
p.add_argument('strip_report', type=Path)
p.add_argument('bindgen', type=Path)
a = p.parse_args()
release = tomllib.loads(a.cargo.read_text())['profile']['release']
prefix = 'CARGO_PROFILE_RELEASE_'
lto = os.environ.get(prefix + 'LTO', release.get('lto', False))
if lto in ('true', 'false'):
    lto = lto == 'true'
raw_cgu = os.environ.get(prefix + 'CODEGEN_UNITS')
cgu = int(release.get('codegen-units', 16) if raw_cgu is None else raw_cgu)
opt = os.environ.get(prefix + 'OPT_LEVEL', release.get('opt-level', 3))
post = json.loads(a.strip_report.read_text())
assert isinstance(lto, bool) or lto in ('thin', 'fat')
assert cgu > 0
print(json.dumps({
    'schema': 1, 'cargoProfile': 'release',
    'release': {'lto': lto, 'codegenUnits': cgu, 'optLevel': str(opt)},
    'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
    'wasmBindgen': subprocess.check_output([str(a.bindgen), '--version'], text=True).strip(),
    'wasmBindgenSha256': hashlib.sha256(a.bindgen.read_bytes()).hexdigest(),
    'compilerFlagHashes': {k: hashlib.sha256(os.environ.get(k, '').encode()).hexdigest() for k in ('RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS')},
    'wasmPostprocess': post,
}))

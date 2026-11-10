#!/usr/bin/env python3
"""Remove optional WASM name/DWARF sections without rewriting executable bytes.

This is a section-framing parser, not a semantic WASM validator. Validate exports,
instantiate and exercise the resulting module separately before distribution.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile

HEADER = b'\0asm\1\0\0\0'


def _u32(data: bytes, pos: int, limit: int | None = None) -> tuple[int, int]:
    end = len(data) if limit is None else limit
    value = 0
    for index in range(5):
        if pos >= end:
            raise ValueError('Truncated u32 LEB128')
        byte = data[pos]
        pos += 1
        if index == 4 and byte > 15:
            raise ValueError('Overflowing u32 LEB128')
        value |= (byte & 127) << (index * 7)
        if byte < 128:
            return value, pos
    raise ValueError('Overlong u32 LEB128')


def sections(data: bytes):
    if data[:8] != HEADER:
        raise ValueError('Expected a version-1 core WASM module')
    pos = 8
    while pos < len(data):
        start = pos
        kind = data[pos]
        size, payload = _u32(data, pos + 1)
        end = payload + size
        if end > len(data):
            raise ValueError('Section payload exceeds module')
        name = None
        if kind == 0:
            count, text = _u32(data, payload, end)
            if text + count > end:
                raise ValueError('Custom-section name exceeds payload')
            try:
                name = data[text:text + count].decode('utf8')
            except UnicodeDecodeError as error:
                raise ValueError('Invalid UTF-8 custom-section name') from error
        yield kind, name, data[start:end]
        pos = end


def is_debug(kind: int, name: str | None) -> bool:
    return kind == 0 and name is not None and (name == 'name' or name.startswith('.debug_'))


def preserved_sections(data: bytes) -> list[bytes]:
    return [raw for kind, name, raw in sections(data) if not is_debug(kind, name)]


def strip_debug(data: bytes) -> tuple[bytes, list[dict]]:
    kept, removed = [], []
    for kind, name, raw in sections(data):
        if is_debug(kind, name):
            removed.append({'name': name, 'bytes': len(raw)})
        else:
            kept.append(raw)
    result = HEADER + b''.join(kept)
    if preserved_sections(data) != preserved_sections(result):
        raise ValueError('Non-debug section preservation failed')
    return result, removed


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('input', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    if args.input.is_symlink() or args.output.is_symlink():
        parser.error('Refuse symbolic-link inputs or outputs')
    before = args.input.read_bytes()
    after, removed = strip_debug(before)
    mode = stat.S_IMODE(args.output.stat().st_mode if args.output.exists() else args.input.stat().st_mode)
    temporary = None
    try:
        # Same-directory transaction file is part of the owned artifact output;
        # it permits atomic replacement even when output is on another device.
        with tempfile.NamedTemporaryFile(dir=args.output.parent, prefix='.wasm-strip-', delete=False) as stream:
            temporary = Path(stream.name)
            os.fchmod(stream.fileno(), mode)
            stream.write(after)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, args.output)
        temporary = None
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    print(json.dumps({'format': 1, 'policy': 'optional-name-dwarf-only',
                      'before_bytes': len(before), 'after_bytes': len(after),
                      'before_sha256': hashlib.sha256(before).hexdigest(),
                      'after_sha256': hashlib.sha256(after).hexdigest(),
                      'removed': removed, 'all_other_section_bytes_preserved': True}))


if __name__ == '__main__':
    main()

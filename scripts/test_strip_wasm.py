import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name('strip_wasm.py')
spec = importlib.util.spec_from_file_location('strip_wasm', SCRIPT)
assert spec is not None and spec.loader is not None
module = importlib.util.module_from_spec(spec)
sys.modules[spec.name] = module
spec.loader.exec_module(module)


def leb(n):
    b = bytearray()
    while n >= 128:
        b.append((n & 127) | 128)
        n >>= 7
    return bytes(b + bytes([n]))


def section(kind, payload):
    return bytes([kind]) + leb(len(payload)) + payload


def custom(name, data=b''):
    name = name.encode()
    return section(0, leb(len(name)) + name + data)


class WasmSections(unittest.TestCase):
    def test_only_named_debug_custom_sections_removed(self):
        core = section(1, b'\0')
        keep = custom('producers', b'compiler') + custom('linking', b'keep') + custom('sourceMappingURL', b'keep')
        source = module.HEADER + custom('name', b'optional') + core + custom('.debug_info', b'dwarf') + keep
        output, removed = module.strip_debug(source)
        self.assertEqual(output, module.HEADER + core + keep)
        self.assertEqual([r['name'] for r in removed], ['name', '.debug_info'])
        self.assertEqual(module.preserved_sections(source), module.preserved_sections(output))

    def test_no_debug_is_exact_identity(self):
        source = module.HEADER + custom('', b'x') + section(1, b'\0')
        self.assertEqual(module.strip_debug(source), (source, []))

    def test_noncanonical_legal_section_length_bytes_preserved(self):
        core = b'\x01\x81\x00\x00'
        source = module.HEADER + custom('name', b'n') + core
        self.assertEqual(module.strip_debug(source)[0], module.HEADER + core)

    def test_large_leb_size(self):
        source = module.HEADER + custom('name', b'n' * 20000) + custom('kept', b'k' * 18000)
        self.assertEqual(module.strip_debug(source)[0], module.HEADER + custom('kept', b'k' * 18000))

    def test_truncated_and_overflow_framing_rejected(self):
        for tail in [b'\0', b'\0\x80', b'\0\x80\x80\x80\x80\x80\0', b'\0\xff\xff\xff\xff\x1f', b'\0\x05\x01x']:
            with self.subTest(tail=tail), self.assertRaises(ValueError):
                module.strip_debug(module.HEADER + tail)

    def test_invalid_custom_name_and_header_rejected(self):
        for source in [b'', b'not wasm', b'\0asm\2\0\0\0', module.HEADER + section(0, b'\x05x'), module.HEADER + section(0, b'\x01\xff')]:
            with self.subTest(source=source), self.assertRaises(ValueError):
                module.strip_debug(source)

    def test_cli_rejection_preserves_output(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d)
            (p / 'bad').write_bytes(b'bad')
            (p / 'out').write_bytes(b'kept')
            result = subprocess.run([sys.executable, str(SCRIPT), str(p / 'bad'), str(p / 'out')], capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual((p / 'out').read_bytes(), b'kept')

    def test_cli_in_place_success(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / 'app.wasm'
            source = module.HEADER + custom('name', b'n') + section(1, b'\0')
            p.write_bytes(source)
            p.chmod(0o644)
            subprocess.run([sys.executable, str(SCRIPT), str(p), str(p)], check=True, capture_output=True)
            self.assertEqual(p.read_bytes(), module.HEADER + section(1, b'\0'))
            self.assertEqual(p.stat().st_mode & 0o777, 0o644)


if __name__ == '__main__':
    unittest.main()

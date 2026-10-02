"""Reads the shared conformance fixtures (../conformance) and checks the language-neutral digests."""
import struct
from pathlib import Path

import pytest

import kore_fileformat as kore

DIR = Path(__file__).parent.parent / 'conformance'
PASSWORD = 'kore-conformance'
FNV_OFFSET, FNV_PRIME, MASK = 0xcbf29ce484222325, 0x100000001b3, 0xFFFFFFFFFFFFFFFF
CANON_NAN = 0x7FF8_0000_0000_0001


def _fnv(h, data):
    for b in data:
        h = ((h ^ b) * FNV_PRIME) & MASK
    return h


def _digest(block):
    out = [f"rows {block.num_rows}"]
    for col in block.columns:
        name = col.dtype.name
        h, nulls = FNV_OFFSET, 0
        for v in col.data:
            if v is None:
                nulls += 1
                h = _fnv(h, b'\x00')
                continue
            h = _fnv(h, b'\x01')
            if name == 'I64':
                h = _fnv(h, struct.pack('<q', v))
            elif name == 'F64':
                bits = CANON_NAN if v != v else struct.unpack('<Q', struct.pack('<d', v))[0]
                h = _fnv(h, struct.pack('<Q', bits))
            elif name == 'BOOL':
                h = _fnv(h, bytes([int(v)]))
            else:
                raw = v.encode('utf-8')
                h = _fnv(h, struct.pack('<I', len(raw)) + raw)
        out.append(f"{col.name} {'i64' if name == 'I64' else 'f64' if name == 'F64' else 'bool' if name == 'BOOL' else 'str'} {nulls} {h:016x}")
    return '\n'.join(out) + '\n'


EXPECTED = (DIR / 'expected.txt').read_text()


@pytest.mark.parametrize('name', ['default', 'shuffle', 'strlen', 'shuffle_strlen', 'versions'])
def test_fixture_decodes_to_expected_digest(name):
    assert _digest(kore.read_file(DIR / f'{name}.kore')) == EXPECTED


def test_encrypted_fixture():
    blob = (DIR / 'encrypted.kore').read_bytes()
    plain = kore.decrypt_aes256(PASSWORD, blob)
    assert _digest(kore._block_from_native(plain)) == EXPECTED
    with pytest.raises(ValueError):
        kore.decrypt_aes256('wrong', blob)


def test_version_log_time_travel():
    log = (DIR / 'versions.kore').read_bytes()
    assert kore.read_at_version(log, 150).num_rows == 2
    assert kore.read_at_version(log, 200).num_rows == 3000
    with pytest.raises(ValueError):
        kore.read_at_version(log, 50)


def test_python_written_native_file_has_the_same_digest(tmp_path):
    # The same logical data written by this module reads back to the same digest.
    block = kore.read_file(DIR / 'default.kore')
    path = tmp_path / 'py.kore'
    kore.write_file(path, block)
    assert _digest(kore.read_file(path)) == EXPECTED


def test_projection_agrees_with_full_read():
    one = kore.read_file(DIR / 'shuffle_strlen.kore', columns=['s'])
    full = _digest(kore.read_file(DIR / 'shuffle_strlen.kore')).splitlines()
    assert _digest(one).splitlines()[1] == next(line for line in full if line.startswith('s '))

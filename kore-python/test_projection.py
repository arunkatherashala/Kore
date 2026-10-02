"""Column projection: read_file(path, columns=...) and to_arrow(path, columns=...)."""
import os
import tempfile
from pathlib import Path

import pytest

import kore_fileformat as kore

FIXTURE = Path(__file__).parent / 'fixtures' / 'legacy_v3.kore'


def _block():
    b = kore.DataBlock()
    b.add_column('i', kore.DataType.I64, [1, None, 3])
    b.add_column('f', kore.DataType.F64, [1.5, 2.5, None])
    b.add_column('s', kore.DataType.STR, ['a', None, 'c'])
    return b


def test_native_projection_order_and_values():
    with tempfile.TemporaryDirectory() as tmpdir:
        path = Path(tmpdir) / 'p.kore'
        kore.write_file(path, _block())
        got = kore.read_file(path, columns=['s', 'i'])
    assert [c.name for c in got.columns] == ['s', 'i']
    assert list(got.get_column('s').data) == ['a', None, 'c']
    assert list(got.get_column('i').data) == [1, None, 3]
    assert got.num_rows == 3


def test_missing_column_is_keyerror():
    with tempfile.TemporaryDirectory() as tmpdir:
        path = Path(tmpdir) / 'p.kore'
        kore.write_file(path, _block())
        with pytest.raises(KeyError):
            kore.read_file(path, columns=['nope'])


def test_projection_on_legacy_layout_file():
    got = kore.read_file(FIXTURE, columns=['b', 'i'])
    assert [c.name for c in got.columns] == ['b', 'i']
    assert list(got.get_column('i').data) == [1, None, 3, 4, 5]
    with pytest.raises(KeyError):
        kore.read_file(FIXTURE, columns=['nope'])


def test_arrow_projection():
    pytest.importorskip('pyarrow')
    with tempfile.TemporaryDirectory() as tmpdir:
        path = Path(tmpdir) / 'p.kore'
        kore.write_file(path, _block())
        t = kore.to_arrow(path, columns=['f', 's'])
        legacy = kore.to_arrow(FIXTURE, columns=['s'])
    assert t.column_names == ['f', 's']
    assert t.column('s').to_pylist() == ['a', None, 'c']
    assert legacy.column_names == ['s']

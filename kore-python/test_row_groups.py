"""Row groups: write_file(row_group_rows=...) and read_file(where=...)."""
import array
import tempfile
from pathlib import Path

import pytest

import kore_fileformat as kore

N = 10_000


def _block():
    b = kore.DataBlock()
    b.add_column('id', kore.DataType.I64, array.array('q', range(N)))
    b.add_column('x', kore.DataType.F64, [None if i % 50 == 7 else i / 4 for i in range(N)])
    # few distinct values early (dictionary) and many later (plain): groups encode differently
    b.add_column('s', kore.DataType.STR, [f"v{i % (5 if i < N // 2 else 400)}" for i in range(N)])
    return b


def test_roundtrip_matches_single_block(tmp_path):
    block = _block()
    grouped, single = tmp_path / 'g.kore', tmp_path / 's.kore'
    kore.write_file(grouped, block, row_group_rows=1000)
    kore.write_file(single, block)
    assert kore.row_group_count(grouped) == 10
    a, b = kore.read_file(grouped), kore.read_file(single)
    for name in ('id', 'x', 's'):
        assert list(a.get_column(name).data) == list(b.get_column(name).data)
    assert b'RowGroups: 1000 rows each' in grouped.read_bytes()[:500]


def test_where_prunes_groups_and_returns_surviving_groups(tmp_path):
    path = tmp_path / 'g.kore'
    kore.write_file(path, _block(), row_group_rows=1000)
    where = ('id', 2500, 3100)
    assert kore.row_groups_matching(path, where) == 2
    got = kore.read_file(path, columns=['id'], where=where)
    ids = list(got.get_column('id').data)
    assert got.num_rows == 2000 and ids[0] == 2000 and ids[-1] == 3999  # groups 2 and 3, unfiltered rows
    assert kore.row_groups_matching(path, ('id', None, -1)) == 0
    assert kore.read_file(path, columns=['id', 'x'], where=('id', 1e9, None)).num_rows == 0
    assert kore.row_groups_matching(path, ('id', None, None)) == 10


def test_where_on_float_column_ignores_nulls(tmp_path):
    path = tmp_path / 'g.kore'
    kore.write_file(path, _block(), row_group_rows=1000)
    # x = i / 4, so group g spans [250 g, 250 g + 249.75]
    assert kore.row_groups_matching(path, ('x', 600, 700)) == 1


def test_errors(tmp_path):
    grouped, single = tmp_path / 'g.kore', tmp_path / 's.kore'
    kore.write_file(grouped, _block(), row_group_rows=1000)
    kore.write_file(single, _block())
    with pytest.raises(KeyError):
        kore.read_file(grouped, where=('nope', 0, 1))
    with pytest.raises(ValueError):
        kore.row_group_count(single)
    # where on a file without groups is harmless: the whole table is one "group" that is returned
    assert kore.read_file(single, where=('id', 0, 1)).num_rows == N


def test_arrow_with_where_and_columns(tmp_path):
    pytest.importorskip('pyarrow')
    path = tmp_path / 'g.kore'
    kore.write_file(path, _block(), row_group_rows=1000)
    t = kore.to_arrow(path, columns=['s', 'id'], where=('id', 0, 10))
    assert t.num_rows == 1000 and t.column_names == ['s', 'id']
    assert t.column('s').to_pylist()[:6] == ['v0', 'v1', 'v2', 'v3', 'v4', 'v0']


def test_env_var_enables_row_groups(tmp_path, monkeypatch):
    monkeypatch.setenv('KORE_ROW_GROUP_ROWS', '2500')
    path = tmp_path / 'e.kore'
    kore.write_file(path, _block())
    monkeypatch.delenv('KORE_ROW_GROUP_ROWS')
    assert kore.row_group_count(path) == 4


def test_empty_table(tmp_path):
    b = kore.DataBlock()
    b.add_column('id', kore.DataType.I64, [])
    path = tmp_path / 'z.kore'
    kore.write_file(path, b, row_group_rows=10)
    assert kore.read_file(path).num_rows == 0

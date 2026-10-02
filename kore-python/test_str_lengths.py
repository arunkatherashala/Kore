"""KORE_STR_LENGTHS=1 writes string columns with a length-prefixed layout (codec 8)."""
import tempfile
from pathlib import Path

import kore_fileformat as kore


def _dates(n):
    # 3360 distinct values, so the column is not dictionary-encoded
    return [f"199{i % 10}-{(i // 10) % 12 + 1:02d}-{(i // 120) % 28 + 1:02d}" if i % 11 else None for i in range(n)]


def test_length_layout_is_smaller_and_lossless(monkeypatch):
    vals = _dates(20000)
    block = kore.DataBlock()
    block.add_column('d', kore.DataType.STR, vals)
    with tempfile.TemporaryDirectory() as tmpdir:
        plain, lens = Path(tmpdir) / 'plain.kore', Path(tmpdir) / 'lens.kore'
        monkeypatch.delenv('KORE_STR_LENGTHS', raising=False)
        kore.write_file(plain, block)
        monkeypatch.setenv('KORE_STR_LENGTHS', '1')
        kore.write_file(lens, block)
        monkeypatch.delenv('KORE_STR_LENGTHS', raising=False)
        assert lens.stat().st_size < plain.stat().st_size
        assert list(kore.read_file(lens).get_column('d').data) == vals
        assert list(kore.read_file(plain).get_column('d').data) == vals

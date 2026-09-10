"""
Tests for the KORE SQL Jupyter kernel.

Tests the kernel standalone (without Jupyter infrastructure) by exercising
the kernel class methods, magic commands, formatters, and error handling.

Run with::

    cd py-kore
    python -m pytest tests/test_kernel.py -v
"""

from __future__ import annotations

import csv
import os
import sys
import tempfile
from unittest.mock import MagicMock, patch

import pytest

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from pykore import SparkSession
from kore_kernel.formatters import (
    datablock_to_html,
    datablock_to_text,
    format_schema,
    suggest_chart,
)


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------


@pytest.fixture()
def spark():
    """Return a fresh KoreSession backed by the MockEngine."""
    session = SparkSession.builder.appName("kernel-test").getOrCreate()
    yield session
    session.stop()


@pytest.fixture()
def csv_file(tmp_path):
    """Write a temp CSV and return its path."""
    path = str(tmp_path / "test_data.csv")
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=["name", "age", "city"])
        writer.writeheader()
        writer.writerow({"name": "Alice", "age": "30", "city": "NY"})
        writer.writerow({"name": "Bob", "age": "25", "city": "SF"})
        writer.writerow({"name": "Charlie", "age": "35", "city": "NY"})
    return path


@pytest.fixture()
def kernel_env(spark, csv_file):
    """Simulate the kernel environment with a pre-loaded table."""
    spark._engine.load_csv(csv_file, "people")
    return spark


# ===========================================================================
# 1. Formatter tests
# ===========================================================================


class TestDatablockToHtml:
    def test_basic_table(self):
        columns = ["name", "age"]
        rows = [{"name": "Alice", "age": "30"}, {"name": "Bob", "age": "25"}]
        html = datablock_to_html(columns, rows)
        assert "<table" in html
        assert "Alice" in html
        assert "Bob" in html
        assert "<th" in html

    def test_empty_rows(self):
        html = datablock_to_html(["col1"], [])
        assert "<table" in html
        assert "<tbody>" in html

    def test_null_values(self):
        rows = [{"x": None, "y": "hello"}]
        html = datablock_to_html(["x", "y"], rows)
        assert "NULL" in html
        assert "hello" in html

    def test_html_escaping(self):
        rows = [{"val": "<script>alert('xss')</script>"}]
        html = datablock_to_html(["val"], rows)
        assert "<script>" not in html
        assert "&lt;script&gt;" in html

    def test_max_rows_truncation(self):
        rows = [{"i": str(i)} for i in range(100)]
        html = datablock_to_html(["i"], rows, max_rows=10)
        assert "10 of 100" in html

    def test_no_truncation_within_limit(self):
        rows = [{"i": str(i)} for i in range(5)]
        html = datablock_to_html(["i"], rows, max_rows=10)
        assert "Showing" not in html


class TestDatablockToText:
    def test_basic_table(self):
        columns = ["name", "age"]
        rows = [{"name": "Alice", "age": "30"}, {"name": "Bob", "age": "25"}]
        text = datablock_to_text(columns, rows)
        assert "Alice" in text
        assert "Bob" in text
        assert "+" in text
        assert "|" in text

    def test_empty_columns(self):
        text = datablock_to_text([], [])
        assert "empty" in text.lower()

    def test_null_values(self):
        rows = [{"x": None}]
        text = datablock_to_text(["x"], rows)
        assert "null" in text

    def test_max_rows_truncation(self):
        rows = [{"i": str(i)} for i in range(50)]
        text = datablock_to_text(["i"], rows, max_rows=5)
        assert "5 of 50" in text

    def test_alignment(self):
        columns = ["name", "score"]
        rows = [{"name": "A", "score": "100"}, {"name": "B", "score": "99"}]
        text = datablock_to_text(columns, rows)
        lines = text.split("\n")
        lengths = [len(l) for l in lines if l.startswith("+")]
        assert len(set(lengths)) == 1


class TestFormatSchema:
    def test_basic_schema(self):
        columns = ["name", "age"]
        dtypes = [("name", "string"), ("age", "bigint")]
        result = format_schema(columns, dtypes)
        assert "root" in result
        assert "name: string" in result
        assert "age: bigint" in result


class TestSuggestChart:
    def test_returns_svg_for_aggregate(self):
        columns = ["city", "count"]
        rows = [
            {"city": "NY", "count": "10"},
            {"city": "SF", "count": "7"},
            {"city": "LA", "count": "5"},
        ]
        chart = suggest_chart(columns, rows)
        assert chart is not None
        assert "<svg" in chart

    def test_returns_none_for_too_few_rows(self):
        columns = ["city", "count"]
        rows = [{"city": "NY", "count": "10"}]
        assert suggest_chart(columns, rows) is None

    def test_returns_none_for_non_numeric(self):
        columns = ["city", "name"]
        rows = [
            {"city": "NY", "name": "Alice"},
            {"city": "SF", "name": "Bob"},
        ]
        assert suggest_chart(columns, rows) is None

    def test_returns_none_for_single_column(self):
        columns = ["x"]
        rows = [{"x": "1"}, {"x": "2"}]
        assert suggest_chart(columns, rows) is None

    def test_returns_none_for_too_many_rows(self):
        columns = ["k", "v"]
        rows = [{"k": str(i), "v": str(i)} for i in range(25)]
        assert suggest_chart(columns, rows) is None


# ===========================================================================
# 2. Kernel instantiation tests (without Jupyter infrastructure)
# ===========================================================================


class TestKernelInit:
    def test_kernel_class_attributes(self):
        try:
            from kore_kernel.kernel import KoreKernel
        except ImportError:
            pytest.skip("ipykernel not installed")
        assert KoreKernel.implementation == "kore"
        assert KoreKernel.language == "sql"
        assert KoreKernel.language_info["name"] == "kore-sql"
        assert KoreKernel.language_info["file_extension"] == ".sql"
        assert KoreKernel.banner == "KORE Engine — Interactive SQL"


# ===========================================================================
# 3. SQL execution tests (using the engine directly)
# ===========================================================================


class TestSqlExecution:
    def test_basic_select(self, kernel_env):
        df = kernel_env.sql("SELECT * FROM people")
        rows = df.collect()
        assert len(rows) == 3
        assert rows[0]["name"] == "Alice"

    def test_select_with_where(self, kernel_env):
        df = kernel_env.sql("SELECT * FROM people WHERE city = 'NY'")
        rows = df.collect()
        assert len(rows) == 2
        assert all(r["city"] == "NY" for r in rows)

    def test_select_with_order(self, kernel_env):
        df = kernel_env.sql("SELECT * FROM people ORDER BY age")
        rows = df.collect()
        ages = [float(r["age"]) for r in rows]
        assert ages == sorted(ages)

    def test_select_with_limit(self, kernel_env):
        df = kernel_env.sql("SELECT * FROM people LIMIT 2")
        rows = df.collect()
        assert len(rows) == 2

    def test_select_columns(self, kernel_env):
        df = kernel_env.sql("SELECT name, city FROM people")
        rows = df.collect()
        assert "name" in rows[0]
        assert "city" in rows[0]


# ===========================================================================
# 4. Magic command tests (logic only, no Jupyter messaging)
# ===========================================================================


class TestMagicCommands:
    def test_tables_lists_registered(self, kernel_env):
        tables = kernel_env.catalog.listTables()
        assert "people" in tables

    def test_schema_shows_columns(self, kernel_env):
        df = kernel_env.table("people")
        dtypes = df.dtypes
        col_names = [name for name, _ in dtypes]
        assert "name" in col_names
        assert "age" in col_names
        assert "city" in col_names

    def test_load_csv_magic(self, spark, tmp_path):
        path = str(tmp_path / "magic_test.csv")
        with open(path, "w", newline="", encoding="utf-8") as fh:
            writer = csv.DictWriter(fh, fieldnames=["x", "y"])
            writer.writeheader()
            writer.writerow({"x": "1", "y": "a"})
            writer.writerow({"x": "2", "y": "b"})

        spark._engine.load_csv(path, "magic_table")
        tables = spark.catalog.listTables()
        assert "magic_table" in tables

        df = spark.sql("SELECT * FROM magic_table")
        rows = df.collect()
        assert len(rows) == 2

    def test_explain_shows_plan(self, kernel_env):
        df = kernel_env.sql("SELECT * FROM people WHERE age > 25")
        sql = df._build_sql()
        assert "SELECT" in sql
        assert "WHERE" in sql

    def test_time_returns_results(self, kernel_env):
        import time
        start = time.perf_counter()
        df = kernel_env.sql("SELECT * FROM people")
        rows = df.collect()
        elapsed = time.perf_counter() - start
        assert elapsed >= 0
        assert len(rows) == 3


# ===========================================================================
# 5. Error handling tests
# ===========================================================================


class TestErrorHandling:
    def test_empty_table_query(self, spark):
        df = spark.sql("SELECT * FROM nonexistent")
        rows = df.collect()
        assert rows == []

    def test_load_parquet_raises_in_mock(self, spark, tmp_path):
        with pytest.raises(NotImplementedError):
            spark._engine.read_parquet(str(tmp_path / "fake.parquet"), "t")


# ===========================================================================
# 6. Integration tests
# ===========================================================================


class TestIntegration:
    def test_full_workflow(self, spark, csv_file):
        spark._engine.load_csv(csv_file, "workflow_test")

        df = spark.sql("SELECT * FROM workflow_test")
        rows = df.collect()
        assert len(rows) == 3

        columns = list(rows[0].keys())
        html = datablock_to_html(columns, rows)
        assert "<table" in html
        assert "Alice" in html

        text = datablock_to_text(columns, rows)
        assert "Alice" in text
        assert "+" in text

    def test_multiple_tables(self, spark, tmp_path):
        path1 = str(tmp_path / "t1.csv")
        path2 = str(tmp_path / "t2.csv")

        with open(path1, "w", newline="", encoding="utf-8") as fh:
            w = csv.DictWriter(fh, fieldnames=["id", "name"])
            w.writeheader()
            w.writerow({"id": "1", "name": "A"})
            w.writerow({"id": "2", "name": "B"})

        with open(path2, "w", newline="", encoding="utf-8") as fh:
            w = csv.DictWriter(fh, fieldnames=["id", "score"])
            w.writeheader()
            w.writerow({"id": "1", "score": "90"})
            w.writerow({"id": "2", "score": "85"})

        spark._engine.load_csv(path1, "names")
        spark._engine.load_csv(path2, "scores")

        tables = spark.catalog.listTables()
        assert "names" in tables
        assert "scores" in tables

    def test_python_exec_context(self, kernel_env):
        """Test that Python code can use the spark session."""
        local_ns: dict = {"spark": kernel_env}
        exec("result = spark.sql('SELECT * FROM people').collect()", {}, local_ns)
        assert len(local_ns["result"]) == 3


# ===========================================================================
# 7. Kernel install module tests
# ===========================================================================


class TestInstallModule:
    def test_kernel_spec_structure(self):
        from kore_kernel.install import KERNEL_SPEC
        assert "argv" in KERNEL_SPEC
        assert "display_name" in KERNEL_SPEC
        assert "language" in KERNEL_SPEC
        assert KERNEL_SPEC["display_name"] == "KORE SQL"
        assert KERNEL_SPEC["language"] == "sql"
        assert "-m" in KERNEL_SPEC["argv"]
        assert "kore_kernel" in KERNEL_SPEC["argv"]
        assert "{connection_file}" in KERNEL_SPEC["argv"]

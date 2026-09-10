"""
Comprehensive tests for the pykore PySpark-compatible API.

All tests use the pure-Python MockEngine so no compiled Rust binary is needed.

Run with::

    cd py-kore
    python -m pytest tests/test_pyspark_compat.py -v
"""

from __future__ import annotations

import csv
import os
import tempfile
from typing import Any

import pytest

import sys

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from pykore import SparkSession, DataFrame, Col, col, lit, F
from pykore.column import _sql_literal, _sql_operand
from pykore.functions import (
    when,
    count,
    sum as F_sum,
    avg,
    mean,
    min as F_min,
    max as F_max,
    count_distinct,
    upper,
    lower,
    trim,
    length,
    substring,
    concat,
    coalesce,
    abs as F_abs,
    round as F_round,
    ceil,
    floor,
    sqrt,
    pow as F_pow,
    row_number,
    rank,
    dense_rank,
    lag,
    lead,
    array,
    size,
    explode,
    Window,
    WindowSpec,
)
from pykore.types import (
    StringType,
    IntegerType,
    LongType,
    FloatType,
    DoubleType,
    BooleanType,
    ArrayType,
    MapType,
    StructType,
    StructField,
    DataType,
    NullType,
    DateType,
    TimestampType,
    DecimalType,
    ByteType,
    ShortType,
    BinaryType,
)
from pykore.writer import DataFrameWriter


# ---------------------------------------------------------------------------
# Fixtures
# ---------------------------------------------------------------------------

@pytest.fixture()
def spark():
    """Return a fresh KoreSession backed by the MockEngine."""
    session = SparkSession.builder.appName("test").getOrCreate()
    yield session
    session.stop()


@pytest.fixture()
def sample_df(spark):
    """A small DataFrame with name/age/city columns."""
    data = [
        {"name": "Alice", "age": "30", "city": "New York"},
        {"name": "Bob", "age": "25", "city": "Boston"},
        {"name": "Charlie", "age": "35", "city": "New York"},
        {"name": "Diana", "age": "28", "city": "Boston"},
    ]
    return spark.createDataFrame(data)


@pytest.fixture()
def numeric_df(spark):
    """DataFrame with numeric values for aggregation tests."""
    data = [
        {"dept": "eng", "salary": "100", "bonus": "10"},
        {"dept": "eng", "salary": "120", "bonus": "15"},
        {"dept": "sales", "salary": "90", "bonus": "20"},
        {"dept": "sales", "salary": "95", "bonus": "12"},
        {"dept": "hr", "salary": "80", "bonus": "8"},
    ]
    return spark.createDataFrame(data)


@pytest.fixture()
def csv_file(tmp_path):
    """Write a temp CSV and return its path."""
    path = str(tmp_path / "people.csv")
    with open(path, "w", newline="", encoding="utf-8") as fh:
        writer = csv.DictWriter(fh, fieldnames=["name", "age", "city"])
        writer.writeheader()
        writer.writerow({"name": "Alice", "age": "30", "city": "NY"})
        writer.writerow({"name": "Bob", "age": "25", "city": "SF"})
    return path


# ═══════════════════════════════════════════════════════════════════════════
# 1. SparkSession / Builder
# ═══════════════════════════════════════════════════════════════════════════

class TestSparkSessionBuilder:
    def test_builder_default(self):
        spark = SparkSession.builder.getOrCreate()
        assert repr(spark) == "KoreSession(app='KORE')"
        spark.stop()

    def test_builder_app_name(self):
        spark = SparkSession.builder.appName("my-app").getOrCreate()
        assert "my-app" in repr(spark)
        spark.stop()

    def test_builder_master(self):
        builder = SparkSession.Builder().master("local[4]")
        assert builder._master == "local[4]"

    def test_builder_config(self):
        builder = SparkSession.Builder().config("spark.sql.shuffle.partitions", "10")
        assert builder._config["spark.sql.shuffle.partitions"] == "10"

    def test_builder_chaining(self):
        spark = (
            SparkSession.Builder()
            .master("local")
            .appName("chain-test")
            .config("k", "v")
            .enableHiveSupport()
            .getOrCreate()
        )
        assert "chain-test" in repr(spark)
        assert spark.conf["k"] == "v"
        spark.stop()

    def test_context_manager(self):
        with SparkSession.builder.appName("ctx").getOrCreate() as spark:
            assert spark is not None
        assert spark._engine is None

    def test_spark_context_returns_self(self, spark):
        assert spark.sparkContext is spark

    def test_conf_dict(self, spark):
        assert isinstance(spark.conf, dict)


# ═══════════════════════════════════════════════════════════════════════════
# 2. DataFrameReader (CSV / Parquet / JSON)
# ═══════════════════════════════════════════════════════════════════════════

class TestDataFrameReader:
    def test_read_csv(self, spark, csv_file):
        df = spark.read.csv(csv_file)
        rows = df.collect()
        assert len(rows) == 2
        assert rows[0]["name"] == "Alice"

    def test_read_format_csv(self, spark, csv_file):
        df = spark.read.format("csv").load(csv_file)
        assert df.count() == 2

    def test_read_option(self, spark, csv_file):
        reader = spark.read.option("header", "true").option("sep", ",")
        assert reader._options["header"] == "true"

    def test_read_options_kwargs(self, spark, csv_file):
        reader = spark.read.options(header="true", sep=",")
        assert reader._options["sep"] == ","

    def test_read_parquet_raises_in_mock(self, spark, tmp_path):
        with pytest.raises(NotImplementedError):
            spark.read.parquet(str(tmp_path / "fake.parquet"))

    def test_read_json_raises(self, spark, tmp_path):
        with pytest.raises(NotImplementedError):
            spark.read.json(str(tmp_path / "fake.json"))

    def test_read_unsupported_format(self, spark, tmp_path):
        with pytest.raises(ValueError, match="Unsupported format"):
            spark.read.format("avro").load(str(tmp_path / "f.avro"))

    def test_reader_table(self, spark, sample_df):
        tables = spark.catalog.listTables()
        assert len(tables) >= 1
        name = tables[0]
        df = spark.read.table(name)
        assert df.count() > 0


# ═══════════════════════════════════════════════════════════════════════════
# 3. createDataFrame
# ═══════════════════════════════════════════════════════════════════════════

class TestCreateDataFrame:
    def test_from_dicts(self, spark):
        data = [{"x": "1", "y": "a"}, {"x": "2", "y": "b"}]
        df = spark.createDataFrame(data)
        assert df.count() == 2

    def test_from_tuples_with_schema(self, spark):
        data = [("Alice", 30), ("Bob", 25)]
        df = spark.createDataFrame(data, schema=["name", "age"])
        assert "name" in df.columns
        assert "age" in df.columns
        assert df.count() == 2

    def test_from_tuples_auto_schema(self, spark):
        data = [(1, 2), (3, 4)]
        df = spark.createDataFrame(data)
        assert df.count() == 2
        assert "_0" in df.columns

    def test_from_struct_type_schema(self, spark):
        schema = StructType([
            StructField("id", IntegerType()),
            StructField("label", StringType()),
        ])
        data = [(1, "a"), (2, "b")]
        df = spark.createDataFrame(data, schema=schema)
        assert "id" in df.columns
        assert df.count() == 2

    def test_empty_data(self, spark):
        df = spark.createDataFrame([])
        assert df.count() == 0


# ═══════════════════════════════════════════════════════════════════════════
# 4. DataFrame transformations (lazy)
# ═══════════════════════════════════════════════════════════════════════════

class TestDataFrameTransformations:
    def test_select_string_cols(self, sample_df):
        df = sample_df.select("name", "city")
        sql = df._build_sql()
        assert "name" in sql
        assert "city" in sql

    def test_select_col_objects(self, sample_df):
        df = sample_df.select(col("name"), col("age"))
        rows = df.collect()
        assert len(rows) == 4

    def test_filter_string(self, sample_df):
        df = sample_df.filter("city = 'New York'")
        rows = df.collect()
        assert all(r["city"] == "New York" for r in rows)

    def test_where_alias(self, sample_df):
        df = sample_df.where("city = 'Boston'")
        rows = df.collect()
        assert all(r["city"] == "Boston" for r in rows)

    def test_filter_col_expression(self, sample_df):
        df = sample_df.filter(col("age") > 28)
        sql = df._build_sql()
        assert "age > 28" in sql

    def test_order_by_asc(self, sample_df):
        df = sample_df.orderBy("name")
        rows = df.collect()
        names = [r["name"] for r in rows]
        assert names == sorted(names)

    def test_order_by_desc(self, sample_df):
        df = sample_df.orderBy("age", ascending=False)
        rows = df.collect()
        ages = [float(r["age"]) for r in rows]
        assert ages == sorted(ages, reverse=True)

    def test_sort_alias(self, sample_df):
        df = sample_df.sort("name")
        rows = df.collect()
        names = [r["name"] for r in rows]
        assert names == sorted(names)

    def test_order_by_multiple_ascending_list(self, sample_df):
        df = sample_df.orderBy("city", "name", ascending=[True, False])
        sql = df._build_sql()
        assert "ASC" in sql
        assert "DESC" in sql

    def test_limit(self, sample_df):
        df = sample_df.limit(2)
        rows = df.collect()
        assert len(rows) == 2

    def test_distinct(self, spark):
        data = [{"v": "a"}, {"v": "a"}, {"v": "b"}]
        df = spark.createDataFrame(data).distinct()
        sql = df._build_sql()
        assert "DISTINCT" in sql.upper()

    def test_drop_duplicates(self, spark):
        data = [{"v": "a"}, {"v": "a"}, {"v": "b"}]
        df = spark.createDataFrame(data).dropDuplicates()
        sql = df._build_sql()
        assert "DISTINCT" in sql.upper()

    def test_drop_duplicates_subset_raises(self, sample_df):
        with pytest.raises(NotImplementedError):
            sample_df.dropDuplicates(subset=["city"])

    def test_with_column(self, sample_df):
        df = sample_df.withColumn("greeting", lit("hello"))
        sql = df._build_sql()
        assert "hello" in sql.lower()
        assert "greeting" in sql.lower()

    def test_with_column_renamed(self, sample_df):
        df = sample_df.withColumnRenamed("name", "full_name")
        sql = df._build_sql()
        assert "full_name" in sql

    def test_drop_column(self, sample_df):
        df = sample_df.drop("city")
        sql = df._build_sql()
        assert "drop" not in sql.lower()

    def test_union(self, spark):
        df1 = spark.createDataFrame([{"a": "1"}])
        df2 = spark.createDataFrame([{"a": "2"}])
        combined = df1.union(df2)
        sql = combined._build_sql()
        assert "UNION ALL" in sql

    def test_union_all_alias(self, spark):
        df1 = spark.createDataFrame([{"a": "1"}])
        df2 = spark.createDataFrame([{"a": "2"}])
        combined = df1.unionAll(df2)
        sql = combined._build_sql()
        assert "UNION ALL" in sql

    def test_intersect(self, spark):
        df1 = spark.createDataFrame([{"a": "1"}, {"a": "2"}])
        df2 = spark.createDataFrame([{"a": "2"}, {"a": "3"}])
        result = df1.intersect(df2)
        sql = result._build_sql()
        assert "INTERSECT" in sql

    def test_subtract(self, spark):
        df1 = spark.createDataFrame([{"a": "1"}, {"a": "2"}])
        df2 = spark.createDataFrame([{"a": "2"}])
        result = df1.subtract(df2)
        sql = result._build_sql()
        assert "EXCEPT" in sql

    def test_except_all_alias(self, spark):
        df1 = spark.createDataFrame([{"a": "1"}])
        df2 = spark.createDataFrame([{"a": "2"}])
        result = df1.exceptAll(df2)
        sql = result._build_sql()
        assert "EXCEPT" in sql

    def test_alias(self, sample_df):
        df = sample_df.alias("people")
        sql = df._build_sql()
        assert "people" in sql

    def test_cache_noop(self, sample_df):
        assert sample_df.cache() is sample_df

    def test_persist_unpersist_noop(self, sample_df):
        assert sample_df.persist() is sample_df
        assert sample_df.unpersist() is sample_df

    def test_coalesce_noop(self, sample_df):
        assert sample_df.coalesce(1) is sample_df

    def test_repartition_noop(self, sample_df):
        assert sample_df.repartition(4) is sample_df

    def test_sample_noop(self, sample_df):
        assert sample_df.sample(fraction=0.5) is sample_df

    def test_getitem_string(self, sample_df):
        c = sample_df["name"]
        assert isinstance(c, Col)
        assert c._expr == "name"

    def test_getitem_list(self, sample_df):
        df = sample_df[["name", "age"]]
        assert isinstance(df, DataFrame)

    def test_getitem_col_filter(self, sample_df):
        df = sample_df[col("age") > 28]
        assert isinstance(df, DataFrame)


# ═══════════════════════════════════════════════════════════════════════════
# 5. Join operations
# ═══════════════════════════════════════════════════════════════════════════

class TestJoins:
    def test_inner_join_sql(self, spark):
        df1 = spark.createDataFrame([{"id": "1", "name": "A"}])
        df2 = spark.createDataFrame([{"id": "1", "score": "90"}])
        joined = df1.join(df2, on="id", how="inner")
        sql = joined._build_sql()
        assert "INNER JOIN" in sql

    def test_left_join_sql(self, spark):
        df1 = spark.createDataFrame([{"id": "1"}])
        df2 = spark.createDataFrame([{"id": "1"}])
        sql = df1.join(df2, on="id", how="left")._build_sql()
        assert "LEFT JOIN" in sql

    def test_right_join_sql(self, spark):
        df1 = spark.createDataFrame([{"id": "1"}])
        df2 = spark.createDataFrame([{"id": "1"}])
        sql = df1.join(df2, on="id", how="right")._build_sql()
        assert "RIGHT JOIN" in sql

    def test_full_join_sql(self, spark):
        df1 = spark.createDataFrame([{"id": "1"}])
        df2 = spark.createDataFrame([{"id": "1"}])
        sql = df1.join(df2, on="id", how="full")._build_sql()
        assert "FULL JOIN" in sql

    def test_cross_join(self, spark):
        df1 = spark.createDataFrame([{"a": "1"}])
        df2 = spark.createDataFrame([{"b": "2"}])
        sql = df1.crossJoin(df2)._build_sql()
        assert "CROSS JOIN" in sql

    def test_join_on_col(self, spark):
        df1 = spark.createDataFrame([{"id": "1"}])
        df2 = spark.createDataFrame([{"id": "1"}])
        c = col("df1.id") == col("df2.id")
        sql = df1.join(df2, on=c)._build_sql()
        assert "JOIN" in sql

    def test_join_on_list(self, spark):
        df1 = spark.createDataFrame([{"id": "1", "k": "x"}])
        df2 = spark.createDataFrame([{"id": "1", "k": "x"}])
        sql = df1.join(df2, on=["id", "k"])._build_sql()
        assert "id" in sql
        assert "k" in sql

    def test_left_outer_join(self, spark):
        df1 = spark.createDataFrame([{"id": "1"}])
        df2 = spark.createDataFrame([{"id": "1"}])
        sql = df1.join(df2, on="id", how="left_outer")._build_sql()
        assert "LEFT JOIN" in sql


# ═══════════════════════════════════════════════════════════════════════════
# 6. GroupedData / aggregations
# ═══════════════════════════════════════════════════════════════════════════

class TestGroupedData:
    def test_group_by_count_sql(self, numeric_df):
        gd = numeric_df.groupBy("dept")
        df = gd.count()
        sql = df._build_sql()
        assert "COUNT(*)" in sql
        assert "GROUP BY" in sql

    def test_group_by_sum_sql(self, numeric_df):
        df = numeric_df.groupBy("dept").sum("salary")
        sql = df._build_sql()
        assert "SUM(salary)" in sql

    def test_group_by_avg_sql(self, numeric_df):
        df = numeric_df.groupBy("dept").avg("salary")
        sql = df._build_sql()
        assert "AVG(salary)" in sql

    def test_group_by_mean_alias(self, numeric_df):
        df = numeric_df.groupBy("dept").mean("salary")
        sql = df._build_sql()
        assert "AVG(salary)" in sql

    def test_group_by_min_sql(self, numeric_df):
        df = numeric_df.groupBy("dept").min("salary")
        sql = df._build_sql()
        assert "MIN(salary)" in sql

    def test_group_by_max_sql(self, numeric_df):
        df = numeric_df.groupBy("dept").max("salary")
        sql = df._build_sql()
        assert "MAX(salary)" in sql

    def test_group_by_agg_dict(self, numeric_df):
        df = numeric_df.groupBy("dept").agg({"salary": "avg", "bonus": "sum"})
        sql = df._build_sql()
        assert "AVG(salary)" in sql
        assert "SUM(bonus)" in sql

    def test_group_by_agg_col_exprs(self, numeric_df):
        df = numeric_df.groupBy("dept").agg(
            F_sum("salary").alias("total_salary"),
            F_max("bonus").alias("max_bonus"),
        )
        sql = df._build_sql()
        assert "SUM(salary)" in sql
        assert "MAX(bonus)" in sql

    def test_groupby_snake_case_alias(self, numeric_df):
        gd = numeric_df.groupby("dept")
        assert isinstance(gd, type(numeric_df.groupBy("dept")))

    def test_pivot_not_implemented(self, numeric_df):
        gd = numeric_df.groupBy("dept")
        with pytest.raises(NotImplementedError):
            gd.pivot("dept")


# ═══════════════════════════════════════════════════════════════════════════
# 7. Actions (eager execution)
# ═══════════════════════════════════════════════════════════════════════════

class TestActions:
    def test_collect(self, sample_df):
        rows = sample_df.collect()
        assert isinstance(rows, list)
        assert len(rows) == 4
        assert isinstance(rows[0], dict)

    def test_count(self, sample_df):
        assert sample_df.count() == 4

    def test_first(self, sample_df):
        row = sample_df.first()
        assert row is not None
        assert "name" in row

    def test_head_default(self, sample_df):
        row = sample_df.head()
        assert isinstance(row, dict)

    def test_head_n(self, sample_df):
        rows = sample_df.head(2)
        assert isinstance(rows, list)
        assert len(rows) == 2

    def test_take(self, sample_df):
        rows = sample_df.take(3)
        assert len(rows) == 3

    def test_show_prints(self, sample_df, capsys):
        sample_df.show()
        captured = capsys.readouterr()
        assert "Alice" in captured.out
        assert "+" in captured.out

    def test_show_empty(self, spark, capsys):
        df = spark.createDataFrame([])
        df.show()
        captured = capsys.readouterr()
        assert "empty" in captured.out.lower()

    def test_show_truncate(self, spark, capsys):
        data = [{"val": "a" * 30}]
        df = spark.createDataFrame(data)
        df.show(truncate=True)
        captured = capsys.readouterr()
        assert "..." in captured.out

    def test_show_no_truncate(self, spark, capsys):
        data = [{"val": "a" * 30}]
        df = spark.createDataFrame(data)
        df.show(truncate=False)
        captured = capsys.readouterr()
        assert "a" * 30 in captured.out

    def test_is_empty(self, spark):
        df = spark.createDataFrame([])
        assert df.isEmpty() is True

    def test_is_not_empty(self, sample_df):
        assert sample_df.isEmpty() is False

    def test_to_local_iterator(self, sample_df):
        it = sample_df.toLocalIterator()
        items = list(it)
        assert len(items) == 4

    def test_foreach(self, sample_df):
        collected = []
        sample_df.foreach(lambda r: collected.append(r))
        assert len(collected) == 4

    def test_foreach_partition(self, sample_df):
        partitions = []
        sample_df.foreachPartition(lambda it: partitions.append(list(it)))
        assert len(partitions) == 1
        assert len(partitions[0]) == 4


# ═══════════════════════════════════════════════════════════════════════════
# 8. Schema / metadata
# ═══════════════════════════════════════════════════════════════════════════

class TestSchemaMetadata:
    def test_columns(self, sample_df):
        cols = sample_df.columns
        assert "name" in cols
        assert "age" in cols
        assert "city" in cols

    def test_dtypes(self, sample_df):
        dt = sample_df.dtypes
        assert isinstance(dt, list)
        assert all(isinstance(t, tuple) and len(t) == 2 for t in dt)

    def test_schema_returns_struct_type(self, sample_df):
        s = sample_df.schema
        assert isinstance(s, StructType)
        assert len(s.fields) > 0

    def test_print_schema(self, sample_df, capsys):
        sample_df.printSchema()
        out = capsys.readouterr().out
        assert "root" in out
        assert "name" in out

    def test_explain(self, sample_df, capsys):
        sample_df.explain()
        out = capsys.readouterr().out
        assert "Physical Plan" in out

    def test_describe_sql(self, sample_df):
        desc = sample_df.describe("age")
        sql = desc._build_sql()
        assert "COUNT" in sql
        assert "AVG" in sql
        assert "MIN" in sql
        assert "MAX" in sql

    def test_repr(self, sample_df):
        r = repr(sample_df)
        assert "KoreDataFrame" in r


# ═══════════════════════════════════════════════════════════════════════════
# 9. Column expressions
# ═══════════════════════════════════════════════════════════════════════════

class TestColumnExpressions:
    def test_col_repr(self):
        c = col("name")
        assert "name" in repr(c)

    def test_col_str(self):
        c = col("age")
        assert str(c) == "age"

    def test_alias(self):
        c = col("name").alias("n")
        assert c._expr == "name AS n"

    def test_name_alias(self):
        c = col("name").name("n")
        assert c._expr == "name AS n"

    def test_asc(self):
        c = col("age").asc()
        assert "ASC" in c._expr

    def test_desc(self):
        c = col("age").desc()
        assert "DESC" in c._expr

    def test_cast_string(self):
        c = col("age").cast("int")
        assert "CAST(age AS INT)" == c._expr

    def test_cast_type_object(self):
        c = col("age").cast(IntegerType())
        assert "CAST" in c._expr
        assert "INTEGER" in c._expr

    def test_astype_alias(self):
        c = col("x").astype("double")
        assert "CAST" in c._expr

    def test_is_null(self):
        c = col("x").isNull()
        assert c._expr == "x IS NULL"

    def test_is_not_null(self):
        c = col("x").isNotNull()
        assert c._expr == "x IS NOT NULL"

    def test_isin(self):
        c = col("status").isin("active", "pending")
        assert "IN" in c._expr
        assert "'active'" in c._expr
        assert "'pending'" in c._expr

    def test_between(self):
        c = col("age").between(18, 65)
        assert "BETWEEN" in c._expr
        assert "18" in c._expr
        assert "65" in c._expr

    def test_like(self):
        c = col("name").like("%lice")
        assert "LIKE '%lice'" in c._expr

    def test_rlike(self):
        c = col("name").rlike("^A.*")
        assert "RLIKE" in c._expr

    def test_startswith(self):
        c = col("name").startswith("Al")
        assert "LIKE 'Al%'" in c._expr

    def test_endswith(self):
        c = col("name").endswith("ce")
        assert "LIKE '%ce'" in c._expr

    def test_contains(self):
        c = col("name").contains("li")
        assert "LIKE '%li%'" in c._expr

    # -- comparison operators --

    def test_eq(self):
        c = col("x") == 5
        assert "x = 5" in c._expr

    def test_ne(self):
        c = col("x") != 5
        assert "x != 5" in c._expr

    def test_gt(self):
        c = col("x") > 5
        assert "x > 5" in c._expr

    def test_lt(self):
        c = col("x") < 5
        assert "x < 5" in c._expr

    def test_ge(self):
        c = col("x") >= 5
        assert "x >= 5" in c._expr

    def test_le(self):
        c = col("x") <= 5
        assert "x <= 5" in c._expr

    # -- arithmetic operators --

    def test_add(self):
        c = col("x") + 1
        assert "(x + 1)" == c._expr

    def test_radd(self):
        c = 1 + col("x")
        assert "(1 + x)" == c._expr

    def test_sub(self):
        c = col("x") - 1
        assert "(x - 1)" == c._expr

    def test_mul(self):
        c = col("x") * 2
        assert "(x * 2)" == c._expr

    def test_truediv(self):
        c = col("x") / 2
        assert "(x / 2)" == c._expr

    def test_mod(self):
        c = col("x") % 3
        assert "(x % 3)" == c._expr

    def test_neg(self):
        c = -col("x")
        assert "(-x)" == c._expr

    # -- boolean operators --

    def test_and(self):
        c = (col("x") > 1) & (col("y") < 10)
        assert "AND" in c._expr

    def test_or(self):
        c = (col("x") > 1) | (col("y") < 10)
        assert "OR" in c._expr

    def test_invert(self):
        c = ~(col("x") > 1)
        assert "NOT" in c._expr

    # -- over (window) --

    def test_over(self):
        w = Window.partitionBy("dept").orderBy("salary")
        c = row_number().over(w)
        assert "OVER" in c._expr
        assert "PARTITION BY" in c._expr

    # -- string comparison with Col --

    def test_eq_string_literal(self):
        c = col("city") == "Boston"
        assert "'Boston'" in c._expr

    def test_eq_none(self):
        c = col("x") == None  # noqa: E711
        assert "NULL" in c._expr


# ═══════════════════════════════════════════════════════════════════════════
# 10. SQL literal / operand helpers
# ═══════════════════════════════════════════════════════════════════════════

class TestSqlHelpers:
    def test_literal_none(self):
        assert _sql_literal(None) == "NULL"

    def test_literal_bool_true(self):
        assert _sql_literal(True) == "TRUE"

    def test_literal_bool_false(self):
        assert _sql_literal(False) == "FALSE"

    def test_literal_string(self):
        assert _sql_literal("hello") == "'hello'"

    def test_literal_string_escape(self):
        assert _sql_literal("it's") == "'it''s'"

    def test_literal_int(self):
        assert _sql_literal(42) == "42"

    def test_literal_float(self):
        assert _sql_literal(3.14) == "3.14"

    def test_operand_col(self):
        c = col("x")
        assert _sql_operand(c) == "x"

    def test_operand_value(self):
        assert _sql_operand(10) == "10"


# ═══════════════════════════════════════════════════════════════════════════
# 11. Functions module (F)
# ═══════════════════════════════════════════════════════════════════════════

class TestFunctions:
    def test_col_function(self):
        c = F.col("name")
        assert c._expr == "name"

    def test_lit_function(self):
        c = F.lit(42)
        assert c._expr == "42"

    def test_lit_string(self):
        c = F.lit("hello")
        assert c._expr == "'hello'"

    # -- conditional --

    def test_when_otherwise(self):
        expr = when(col("age") > 30, "senior").otherwise("junior")
        assert "CASE" in expr._expr
        assert "WHEN" in expr._expr
        assert "ELSE" in expr._expr
        assert "END" in expr._expr

    def test_when_chained(self):
        expr = (
            when(col("age") > 60, "senior")
            .when(col("age") > 30, "mid")
            .otherwise("junior")
        )
        assert expr._expr.count("WHEN") == 2

    # -- aggregates --

    def test_count_star(self):
        assert count()._expr == "COUNT(*)"

    def test_count_col(self):
        assert count("x")._expr == "COUNT(x)"

    def test_sum(self):
        assert F_sum("salary")._expr == "SUM(salary)"

    def test_avg(self):
        assert avg("salary")._expr == "AVG(salary)"

    def test_mean(self):
        assert mean("salary")._expr == "AVG(salary)"

    def test_min(self):
        assert F_min("age")._expr == "MIN(age)"

    def test_max(self):
        assert F_max("age")._expr == "MAX(age)"

    def test_count_distinct(self):
        c = count_distinct("city")
        assert "COUNT(DISTINCT" in c._expr

    # -- string functions --

    def test_upper(self):
        assert upper("name")._expr == "UPPER(name)"

    def test_lower(self):
        assert lower("name")._expr == "LOWER(name)"

    def test_trim(self):
        assert trim("name")._expr == "TRIM(name)"

    def test_length(self):
        assert length("name")._expr == "LENGTH(name)"

    def test_substring(self):
        c = substring("name", 1, 3)
        assert "SUBSTRING(name, 1, 3)" == c._expr

    def test_concat(self):
        c = concat(col("first"), lit(" "), col("last"))
        assert "CONCAT" in c._expr

    def test_coalesce(self):
        c = coalesce(col("a"), col("b"), lit(0))
        assert "COALESCE" in c._expr

    # -- math functions --

    def test_abs(self):
        assert F_abs("x")._expr == "ABS(x)"

    def test_round(self):
        c = F_round("x", 2)
        assert "ROUND(x, 2)" == c._expr

    def test_ceil(self):
        assert ceil("x")._expr == "CEIL(x)"

    def test_floor(self):
        assert floor("x")._expr == "FLOOR(x)"

    def test_sqrt(self):
        assert sqrt("x")._expr == "SQRT(x)"

    def test_pow(self):
        c = F_pow("x", 3)
        assert "POWER(x, 3)" == c._expr

    # -- window functions --

    def test_row_number(self):
        assert row_number()._expr == "ROW_NUMBER()"

    def test_rank(self):
        assert rank()._expr == "RANK()"

    def test_dense_rank(self):
        assert dense_rank()._expr == "DENSE_RANK()"

    def test_lag(self):
        c = lag("salary", 1)
        assert "LAG(salary, 1)" == c._expr

    def test_lag_with_default(self):
        c = lag("salary", 1, 0)
        assert "LAG(salary, 1, 0)" == c._expr

    def test_lead(self):
        c = lead("salary", 2)
        assert "LEAD(salary, 2)" == c._expr

    # -- array functions --

    def test_array(self):
        c = array(col("a"), col("b"))
        assert "ARRAY(a, b)" == c._expr

    def test_size(self):
        assert size("arr")._expr == "SIZE(arr)"

    def test_explode(self):
        assert explode("arr")._expr == "EXPLODE(arr)"


# ═══════════════════════════════════════════════════════════════════════════
# 12. Window class
# ═══════════════════════════════════════════════════════════════════════════

class TestWindow:
    def test_partition_by(self):
        w = Window.partitionBy("dept")
        assert "PARTITION BY dept" in str(w)

    def test_order_by(self):
        w = Window.orderBy("salary")
        assert "ORDER BY salary" in str(w)

    def test_partition_and_order(self):
        w = Window.partitionBy("dept").orderBy("salary")
        s = str(w)
        assert "PARTITION BY dept" in s
        assert "ORDER BY salary" in s

    def test_rows_between(self):
        w = Window.partitionBy("dept").orderBy("salary").rowsBetween(-1, 1)
        s = str(w)
        assert "ROWS BETWEEN" in s
        assert "1 PRECEDING" in s
        assert "1 FOLLOWING" in s

    def test_range_between(self):
        w = Window.orderBy("ts").rangeBetween(
            Window.unboundedPreceding, Window.currentRow
        )
        s = str(w)
        assert "UNBOUNDED PRECEDING" in s
        assert "CURRENT ROW" in s

    def test_unbounded_following(self):
        w = Window.orderBy("ts").rowsBetween(0, Window.unboundedFollowing)
        s = str(w)
        assert "CURRENT ROW" in s
        assert "UNBOUNDED FOLLOWING" in s

    def test_window_spec_chain_immutable(self):
        w1 = Window.partitionBy("a")
        w2 = w1.orderBy("b")
        assert "ORDER BY" not in str(w1)
        assert "ORDER BY" in str(w2)


# ═══════════════════════════════════════════════════════════════════════════
# 13. Data types
# ═══════════════════════════════════════════════════════════════════════════

class TestDataTypes:
    def test_string_type(self):
        t = StringType()
        assert t.typeName() == "string"
        assert t.simpleString() == "string"

    def test_integer_type(self):
        t = IntegerType()
        assert t.typeName() == "integer"

    def test_long_type(self):
        assert LongType().typeName() == "long"

    def test_float_type(self):
        assert FloatType().typeName() == "float"

    def test_double_type(self):
        assert DoubleType().typeName() == "double"

    def test_boolean_type(self):
        assert BooleanType().typeName() == "boolean"

    def test_null_type(self):
        assert NullType().typeName() == "null"

    def test_date_type(self):
        assert DateType().typeName() == "date"

    def test_timestamp_type(self):
        assert TimestampType().typeName() == "timestamp"

    def test_byte_type(self):
        assert ByteType().typeName() == "byte"

    def test_short_type(self):
        assert ShortType().typeName() == "short"

    def test_binary_type(self):
        assert BinaryType().typeName() == "binary"

    def test_decimal_type(self):
        t = DecimalType(18, 2)
        assert t.precision == 18
        assert t.scale == 2
        assert "decimal" in t.simpleString()

    def test_array_type(self):
        t = ArrayType(StringType())
        assert "array<string>" == t.simpleString()

    def test_map_type(self):
        t = MapType(StringType(), IntegerType())
        assert "map<string,integer>" == t.simpleString()

    def test_struct_type(self):
        st = StructType([
            StructField("id", IntegerType()),
            StructField("name", StringType()),
        ])
        assert st.names == ["id", "name"]
        assert len(st) == 2

    def test_struct_type_add(self):
        st = StructType()
        st.add("id", IntegerType())
        st.add("label", "string")
        assert len(st) == 2
        assert st.names == ["id", "label"]

    def test_struct_type_add_field(self):
        st = StructType()
        st.add(StructField("x", DoubleType()))
        assert st[0].name == "x"

    def test_struct_type_getitem_int(self):
        st = StructType([StructField("a", StringType())])
        assert st[0].name == "a"

    def test_struct_type_getitem_str(self):
        st = StructType([StructField("a", StringType())])
        assert st["a"].name == "a"

    def test_struct_type_getitem_missing(self):
        st = StructType([StructField("a", StringType())])
        with pytest.raises(KeyError):
            st["z"]

    def test_struct_type_iter(self):
        st = StructType([StructField("a", StringType()), StructField("b", IntegerType())])
        names = [f.name for f in st]
        assert names == ["a", "b"]

    def test_data_type_equality(self):
        assert StringType() == StringType()
        assert StringType() != IntegerType()

    def test_data_type_hash(self):
        assert hash(StringType()) == hash(StringType())

    def test_data_type_repr(self):
        assert "StringType()" == repr(StringType())

    def test_struct_field_repr(self):
        sf = StructField("id", IntegerType(), nullable=False)
        r = repr(sf)
        assert "id" in r
        assert "IntegerType" in r

    def test_struct_type_simple_string(self):
        st = StructType([StructField("x", LongType())])
        assert "struct<" in st.simpleString()

    def test_data_type_json(self):
        assert StringType().json() == '"string"'


# ═══════════════════════════════════════════════════════════════════════════
# 14. DataFrameWriter
# ═══════════════════════════════════════════════════════════════════════════

class TestDataFrameWriter:
    def test_writer_format(self, sample_df):
        w = sample_df.write.format("csv")
        assert w._format == "csv"

    def test_writer_mode(self, sample_df):
        w = sample_df.write.mode("overwrite")
        assert w._mode == "overwrite"

    def test_writer_option(self, sample_df):
        w = sample_df.write.option("header", "true")
        assert w._options["header"] == "true"

    def test_writer_options_kwargs(self, sample_df):
        w = sample_df.write.options(header="true", sep="|")
        assert w._options["sep"] == "|"

    def test_writer_partition_by(self, sample_df):
        w = sample_df.write.partitionBy("city")
        assert w._partition_by == ["city"]

    def test_writer_parquet_raises_mock(self, sample_df, tmp_path):
        with pytest.raises(NotImplementedError):
            sample_df.write.parquet(str(tmp_path / "out.parquet"))

    def test_writer_csv_raises_mock(self, sample_df, tmp_path):
        with pytest.raises(NotImplementedError):
            sample_df.write.csv(str(tmp_path / "out.csv"))

    def test_writer_json_raises_mock(self, sample_df, tmp_path):
        with pytest.raises(NotImplementedError):
            sample_df.write.json(str(tmp_path / "out.json"))

    def test_writer_unsupported_format(self, sample_df, tmp_path):
        with pytest.raises(ValueError, match="Unsupported"):
            sample_df.write.format("avro").save(str(tmp_path / "out"))

    def test_write_returns_writer(self, sample_df):
        assert isinstance(sample_df.write, DataFrameWriter)


# ═══════════════════════════════════════════════════════════════════════════
# 15. Catalog
# ═══════════════════════════════════════════════════════════════════════════

class TestCatalog:
    def test_list_tables_empty(self, spark):
        tables = spark.catalog.listTables()
        assert isinstance(tables, list)

    def test_list_tables_after_create(self, spark, sample_df):
        tables = spark.catalog.listTables()
        assert len(tables) >= 1

    def test_table_exists(self, spark, sample_df):
        tables = spark.catalog.listTables()
        if tables:
            assert spark.catalog.tableExists(tables[0])

    def test_table_not_exists(self, spark):
        assert spark.catalog.tableExists("nonexistent_table_xyz") is False


# ═══════════════════════════════════════════════════════════════════════════
# 16. SQL execution
# ═══════════════════════════════════════════════════════════════════════════

class TestSqlExecution:
    def test_sql_select(self, spark, sample_df):
        table_name = spark.catalog.listTables()[0]
        df = spark.sql(f"SELECT * FROM {table_name}")
        rows = df.collect()
        assert len(rows) == 4

    def test_sql_with_where(self, spark, sample_df):
        table_name = spark.catalog.listTables()[0]
        df = spark.sql(f"SELECT * FROM {table_name} WHERE city = 'Boston'")
        rows = df.collect()
        assert all(r["city"] == "Boston" for r in rows)

    def test_table_method(self, spark, sample_df):
        table_name = spark.catalog.listTables()[0]
        df = spark.table(table_name)
        assert df.count() == 4


# ═══════════════════════════════════════════════════════════════════════════
# 17. End-to-end integration
# ═══════════════════════════════════════════════════════════════════════════

class TestEndToEnd:
    def test_csv_read_filter_select_collect(self, spark, csv_file):
        df = spark.read.csv(csv_file)
        result = df.filter("age > 20").select("name").collect()
        assert len(result) >= 1

    def test_create_transform_chain(self, spark):
        data = [
            {"product": "A", "price": "10", "qty": "5"},
            {"product": "B", "price": "20", "qty": "3"},
            {"product": "A", "price": "15", "qty": "2"},
        ]
        df = spark.createDataFrame(data)
        result = (
            df.filter("price > 5")
            .select("product", "price")
            .orderBy("price")
            .limit(2)
            .collect()
        )
        assert len(result) <= 2

    def test_build_sql_produces_valid_structure(self, sample_df):
        sql = (
            sample_df
            .select("name", "city")
            .filter("age > 25")
            .orderBy("name")
            .limit(10)
            ._build_sql()
        )
        assert "SELECT" in sql
        assert "WHERE" in sql
        assert "ORDER BY" in sql
        assert "LIMIT" in sql

    def test_chained_operations_immutable(self, sample_df):
        df1 = sample_df.filter("age > 25")
        df2 = sample_df.filter("age < 30")
        sql1 = df1._build_sql()
        sql2 = df2._build_sql()
        assert "25" in sql1
        assert "30" in sql2
        assert "30" not in sql1
        assert "25" not in sql2


# ═══════════════════════════════════════════════════════════════════════════
# 18. Additional functions coverage
# ═══════════════════════════════════════════════════════════════════════════

class TestAdditionalFunctions:
    def test_concat_ws(self):
        from pykore.functions import concat_ws
        c = concat_ws("-", col("a"), col("b"))
        assert "CONCAT_WS('-', a, b)" == c._expr

    def test_regexp_replace(self):
        from pykore.functions import regexp_replace
        c = regexp_replace("name", "\\d+", "")
        assert "REGEXP_REPLACE" in c._expr

    def test_split(self):
        from pykore.functions import split
        c = split("name", ",")
        assert "SPLIT" in c._expr

    def test_initcap(self):
        from pykore.functions import initcap
        assert initcap("name")._expr == "INITCAP(name)"

    def test_reverse(self):
        from pykore.functions import reverse
        assert reverse("name")._expr == "REVERSE(name)"

    def test_lpad_rpad(self):
        from pykore.functions import lpad, rpad
        assert "LPAD" in lpad("x", 5, "0")._expr
        assert "RPAD" in rpad("x", 5, "0")._expr

    def test_ltrim_rtrim(self):
        from pykore.functions import ltrim, rtrim
        assert ltrim("x")._expr == "LTRIM(x)"
        assert rtrim("x")._expr == "RTRIM(x)"

    def test_ifnull(self):
        from pykore.functions import ifnull
        c = ifnull(col("a"), lit(0))
        assert "IFNULL" in c._expr

    def test_nullif(self):
        from pykore.functions import nullif
        c = nullif(col("a"), lit(0))
        assert "NULLIF" in c._expr

    def test_isnull(self):
        from pykore.functions import isnull
        c = isnull(col("x"))
        assert "IS NULL" in c._expr

    def test_isnan(self):
        from pykore.functions import isnan
        assert isnan("x")._expr == "ISNAN(x)"

    def test_log_functions(self):
        from pykore.functions import log, log2, log10, exp
        assert log("x")._expr == "LN(x)"
        assert log2("x")._expr == "LOG2(x)"
        assert log10("x")._expr == "LOG10(x)"
        assert exp("x")._expr == "EXP(x)"

    def test_date_functions(self):
        from pykore.functions import (
            current_date, current_timestamp, year, month,
            dayofmonth, hour, minute, second,
            date_add, date_sub, datediff,
        )
        assert current_date()._expr == "CURRENT_DATE"
        assert current_timestamp()._expr == "CURRENT_TIMESTAMP"
        assert year("d")._expr == "YEAR(d)"
        assert month("d")._expr == "MONTH(d)"
        assert dayofmonth("d")._expr == "DAY(d)"
        assert hour("t")._expr == "HOUR(t)"
        assert minute("t")._expr == "MINUTE(t)"
        assert second("t")._expr == "SECOND(t)"
        assert "DATE_ADD" in date_add("d", 5)._expr
        assert "DATE_SUB" in date_sub("d", 5)._expr
        assert "DATEDIFF" in datediff("d1", "d2")._expr

    def test_sum_distinct(self):
        from pykore.functions import sumDistinct
        c = sumDistinct("x")
        assert "SUM(DISTINCT" in c._expr

    def test_first_last(self):
        from pykore.functions import first, last
        assert first("x")._expr == "FIRST(x)"
        assert last("x")._expr == "LAST(x)"

    def test_ntile(self):
        from pykore.functions import ntile
        assert ntile(4)._expr == "NTILE(4)"

    def test_cume_dist_percent_rank(self):
        from pykore.functions import cume_dist, percent_rank
        assert cume_dist()._expr == "CUME_DIST()"
        assert percent_rank()._expr == "PERCENT_RANK()"

    def test_array_contains(self):
        from pykore.functions import array_contains
        c = array_contains(col("arr"), "x")
        assert "ARRAY_CONTAINS" in c._expr

    def test_sort_array(self):
        from pykore.functions import sort_array
        c = sort_array(col("arr"))
        assert "SORT_ARRAY" in c._expr

    def test_struct(self):
        from pykore.functions import struct
        c = struct(col("a"), col("b"))
        assert "STRUCT(a, b)" == c._expr

    def test_monotonically_increasing_id(self):
        from pykore.functions import monotonically_increasing_id
        assert "MONOTONICALLY_INCREASING_ID" in monotonically_increasing_id()._expr

    def test_spark_partition_id(self):
        from pykore.functions import spark_partition_id
        assert "SPARK_PARTITION_ID" in spark_partition_id()._expr

    def test_input_file_name(self):
        from pykore.functions import input_file_name
        assert "INPUT_FILE_NAME" in input_file_name()._expr

    def test_power_alias(self):
        from pykore.functions import power
        c = power("x", 2)
        assert "POWER(x, 2)" == c._expr

    def test_day_alias(self):
        from pykore.functions import day
        assert day("d")._expr == "DAY(d)"

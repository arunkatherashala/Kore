"""Quick validation of pykore modules — no external dependencies required."""

import sys
import os
import csv
import io
import contextlib
import tempfile
import traceback

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))

from pykore import SparkSession, DataFrame, Col, col, lit, F
from pykore.column import _sql_literal, _sql_operand
from pykore.functions import (
    when, count, sum as F_sum, avg, mean, min as F_min, max as F_max,
    count_distinct, upper, lower, trim, length, substring, concat, coalesce,
    abs as F_abs, round as F_round, ceil, floor, sqrt, pow as F_pow,
    row_number, rank, dense_rank, lag, lead, array, size, explode,
    Window, WindowSpec, concat_ws, regexp_replace, split, initcap, reverse,
    lpad, rpad, ltrim, rtrim, ifnull, nullif, isnull, isnan, log, log2,
    log10, exp, current_date, current_timestamp, year, month, dayofmonth,
    hour, minute, second, date_add, date_sub, datediff, sumDistinct, first,
    last, ntile, cume_dist, percent_rank, array_contains, sort_array, struct,
    monotonically_increasing_id, spark_partition_id, input_file_name, power, day,
)
from pykore.types import (
    StringType, IntegerType, LongType, FloatType, DoubleType, BooleanType,
    ArrayType, MapType, StructType, StructField, DataType, NullType,
    DateType, TimestampType, DecimalType, ByteType, ShortType, BinaryType,
)
from pykore.writer import DataFrameWriter

passed = 0
failed = 0


def check(name, condition):
    global passed, failed
    if condition:
        passed += 1
        print(f"  PASS: {name}")
    else:
        failed += 1
        print(f"  FAIL: {name}")


def section(title):
    print(f"\n=== {title} ===")


# -------------------------------------------------------------------
section("1. SparkSession Builder")
spark = SparkSession.builder.appName("test").master("local").config("k", "v").getOrCreate()
check("repr", "test" in repr(spark))
check("conf", spark.conf["k"] == "v")
check("sparkContext is self", spark.sparkContext is spark)

b2 = SparkSession.Builder().master("local[4]")
check("master stored", b2._master == "local[4]")

b3 = SparkSession.Builder().config("spark.sql.shuffle.partitions", "10")
check("config stored", b3._config["spark.sql.shuffle.partitions"] == "10")

spark2 = (
    SparkSession.Builder()
    .master("local")
    .appName("chain-test")
    .config("k", "v")
    .enableHiveSupport()
    .getOrCreate()
)
check("chaining", "chain-test" in repr(spark2))
spark2.stop()

# -------------------------------------------------------------------
section("2. createDataFrame")
data = [
    {"name": "Alice", "age": "30", "city": "New York"},
    {"name": "Bob", "age": "25", "city": "Boston"},
    {"name": "Charlie", "age": "35", "city": "New York"},
    {"name": "Diana", "age": "28", "city": "Boston"},
]
df = spark.createDataFrame(data)
check("count=4", df.count() == 4)
check("columns", "name" in df.columns and "age" in df.columns and "city" in df.columns)

df_tuples = spark.createDataFrame([("Alice", 30), ("Bob", 25)], schema=["name", "age"])
check("tuples+schema", "name" in df_tuples.columns and df_tuples.count() == 2)

df_auto = spark.createDataFrame([(1, 2), (3, 4)])
check("auto schema", "_0" in df_auto.columns and df_auto.count() == 2)

schema_st = StructType([StructField("id", IntegerType()), StructField("label", StringType())])
df_st = spark.createDataFrame([(1, "a"), (2, "b")], schema=schema_st)
check("StructType schema", "id" in df_st.columns and df_st.count() == 2)

df_empty = spark.createDataFrame([])
check("empty data", df_empty.count() == 0)

# -------------------------------------------------------------------
section("3. Actions (collect/first/head/take/show)")
rows = df.collect()
check("collect", isinstance(rows, list) and len(rows) == 4 and isinstance(rows[0], dict))
check("first", df.first() is not None and "name" in df.first())
check("head(1)", isinstance(df.head(), dict))
check("head(2)", isinstance(df.head(2), list) and len(df.head(2)) == 2)
check("take(3)", len(df.take(3)) == 3)
check("isEmpty false", df.isEmpty() is False)
check("isEmpty true", df_empty.isEmpty() is True)

buf = io.StringIO()
with contextlib.redirect_stdout(buf):
    df.show()
output = buf.getvalue()
check("show output", "Alice" in output and "+" in output)

buf2 = io.StringIO()
with contextlib.redirect_stdout(buf2):
    df_empty.show()
check("show empty", "empty" in buf2.getvalue().lower())

long_data = [{"val": "a" * 30}]
df_long = spark.createDataFrame(long_data)
buf3 = io.StringIO()
with contextlib.redirect_stdout(buf3):
    df_long.show(truncate=True)
check("show truncate", "..." in buf3.getvalue())

buf4 = io.StringIO()
with contextlib.redirect_stdout(buf4):
    df_long.show(truncate=False)
check("show no truncate", "a" * 30 in buf4.getvalue())

collected_iter = list(df.toLocalIterator())
check("toLocalIterator", len(collected_iter) == 4)

collected_fe = []
df.foreach(lambda r: collected_fe.append(r))
check("foreach", len(collected_fe) == 4)

partitions = []
df.foreachPartition(lambda it: partitions.append(list(it)))
check("foreachPartition", len(partitions) == 1 and len(partitions[0]) == 4)

# -------------------------------------------------------------------
section("4. Transformations (filter/select/orderBy/limit/distinct)")
filtered = df.filter("city = 'New York'")
frows = filtered.collect()
check("filter", all(r["city"] == "New York" for r in frows))

where_rows = df.where("city = 'Boston'").collect()
check("where alias", all(r["city"] == "Boston" for r in where_rows))

filt_col = df.filter(col("age") > 28)
check("filter col expr", "age > 28" in filt_col._build_sql())

sel = df.select("name", "city")
check("select str", "name" in sel._build_sql() and "city" in sel._build_sql())

sel_col = df.select(col("name"), col("age"))
check("select Col", len(sel_col.collect()) == 4)

ordered = df.orderBy("name")
orows = ordered.collect()
names = [r["name"] for r in orows]
check("orderBy asc", names == sorted(names))

ordered_desc = df.orderBy("age", ascending=False)
odrows = ordered_desc.collect()
ages = [float(r["age"]) for r in odrows]
check("orderBy desc", ages == sorted(ages, reverse=True))

sorted_df = df.sort("name")
check("sort alias", [r["name"] for r in sorted_df.collect()] == sorted([r["name"] for r in sorted_df.collect()]))

multi_ord = df.orderBy("city", "name", ascending=[True, False])
check("orderBy multi asc list", "ASC" in multi_ord._build_sql() and "DESC" in multi_ord._build_sql())

check("limit", len(df.limit(2).collect()) == 2)

dist_data = [{"v": "a"}, {"v": "a"}, {"v": "b"}]
dist_df = spark.createDataFrame(dist_data).distinct()
check("distinct", "DISTINCT" in dist_df._build_sql().upper())

dd_df = spark.createDataFrame(dist_data).dropDuplicates()
check("dropDuplicates", "DISTINCT" in dd_df._build_sql().upper())

try:
    df.dropDuplicates(subset=["city"])
    check("dropDuplicates subset raises", False)
except NotImplementedError:
    check("dropDuplicates subset raises", True)

# -------------------------------------------------------------------
section("5. withColumn / withColumnRenamed / drop")
wc = df.withColumn("greeting", lit("hello"))
check("withColumn", "hello" in wc._build_sql().lower() and "greeting" in wc._build_sql().lower())

wr = df.withColumnRenamed("name", "full_name")
check("withColumnRenamed", "full_name" in wr._build_sql())

drp = df.drop("city")
check("drop", "drop" not in drp._build_sql().lower())

# -------------------------------------------------------------------
section("6. Set operations (union/intersect/subtract)")
df1 = spark.createDataFrame([{"a": "1"}])
df2 = spark.createDataFrame([{"a": "2"}])
check("union", "UNION ALL" in df1.union(df2)._build_sql())
check("unionAll alias", "UNION ALL" in df1.unionAll(df2)._build_sql())
check("intersect", "INTERSECT" in df1.intersect(df2)._build_sql())
check("subtract", "EXCEPT" in df1.subtract(df2)._build_sql())
check("exceptAll alias", "EXCEPT" in df1.exceptAll(df2)._build_sql())

# -------------------------------------------------------------------
section("7. Join operations")
jdf1 = spark.createDataFrame([{"id": "1", "name": "A"}])
jdf2 = spark.createDataFrame([{"id": "1", "score": "90"}])
check("inner join", "INNER JOIN" in jdf1.join(jdf2, on="id", how="inner")._build_sql())
check("left join", "LEFT JOIN" in jdf1.join(jdf2, on="id", how="left")._build_sql())
check("right join", "RIGHT JOIN" in jdf1.join(jdf2, on="id", how="right")._build_sql())
check("full join", "FULL JOIN" in jdf1.join(jdf2, on="id", how="full")._build_sql())
check("cross join", "CROSS JOIN" in jdf1.crossJoin(jdf2)._build_sql())
check("left_outer join", "LEFT JOIN" in jdf1.join(jdf2, on="id", how="left_outer")._build_sql())

jc = col("df1.id") == col("df2.id")
check("join on Col", "JOIN" in jdf1.join(jdf2, on=jc)._build_sql())

jlist = spark.createDataFrame([{"id": "1", "k": "x"}])
jlist2 = spark.createDataFrame([{"id": "1", "k": "x"}])
jsql = jlist.join(jlist2, on=["id", "k"])._build_sql()
check("join on list", "id" in jsql and "k" in jsql)

# -------------------------------------------------------------------
section("8. GroupedData / Aggregations")
num_data = [
    {"dept": "eng", "salary": "100", "bonus": "10"},
    {"dept": "eng", "salary": "120", "bonus": "15"},
    {"dept": "sales", "salary": "90", "bonus": "20"},
    {"dept": "sales", "salary": "95", "bonus": "12"},
    {"dept": "hr", "salary": "80", "bonus": "8"},
]
ndf = spark.createDataFrame(num_data)
gd = ndf.groupBy("dept")

cnt_sql = gd.count()._build_sql()
check("group count", "COUNT(*)" in cnt_sql and "GROUP BY" in cnt_sql)
check("group sum", "SUM(salary)" in gd.sum("salary")._build_sql())
check("group avg", "AVG(salary)" in gd.avg("salary")._build_sql())
check("group mean", "AVG(salary)" in gd.mean("salary")._build_sql())
check("group min", "MIN(salary)" in gd.min("salary")._build_sql())
check("group max", "MAX(salary)" in gd.max("salary")._build_sql())

agg_dict = gd.agg({"salary": "avg", "bonus": "sum"})._build_sql()
check("agg dict", "AVG(salary)" in agg_dict and "SUM(bonus)" in agg_dict)

agg_col = gd.agg(F_sum("salary").alias("total_salary"), F_max("bonus").alias("max_bonus"))._build_sql()
check("agg col exprs", "SUM(salary)" in agg_col and "MAX(bonus)" in agg_col)

check("groupby alias", isinstance(ndf.groupby("dept"), type(ndf.groupBy("dept"))))

try:
    gd.pivot("dept")
    check("pivot raises", False)
except NotImplementedError:
    check("pivot raises", True)

# -------------------------------------------------------------------
section("9. Schema / Metadata")
check("columns", "name" in df.columns)
dt = df.dtypes
check("dtypes", isinstance(dt, list) and all(isinstance(t, tuple) and len(t) == 2 for t in dt))

s = df.schema
check("schema StructType", isinstance(s, StructType) and len(s.fields) > 0)

buf5 = io.StringIO()
with contextlib.redirect_stdout(buf5):
    df.printSchema()
check("printSchema", "root" in buf5.getvalue() and "name" in buf5.getvalue())

buf6 = io.StringIO()
with contextlib.redirect_stdout(buf6):
    df.explain()
check("explain", "Physical Plan" in buf6.getvalue())

desc = df.describe("age")
dsql = desc._build_sql()
check("describe", "COUNT" in dsql and "AVG" in dsql and "MIN" in dsql and "MAX" in dsql)

check("repr", "KoreDataFrame" in repr(df))

# -------------------------------------------------------------------
section("10. Column expressions")
check("col repr", "name" in repr(col("name")))
check("col str", str(col("age")) == "age")
check("alias", col("name").alias("n")._expr == "name AS n")
check("name alias", col("name").name("n")._expr == "name AS n")
check("asc", "ASC" in col("age").asc()._expr)
check("desc", "DESC" in col("age").desc()._expr)
check("cast str", col("age").cast("int")._expr == "CAST(age AS INT)")
check("cast type", "CAST" in col("age").cast(IntegerType())._expr and "INTEGER" in col("age").cast(IntegerType())._expr)
check("astype", "CAST" in col("x").astype("double")._expr)
check("isNull", col("x").isNull()._expr == "x IS NULL")
check("isNotNull", col("x").isNotNull()._expr == "x IS NOT NULL")
check("isin", "IN" in col("s").isin("a", "b")._expr and "'a'" in col("s").isin("a", "b")._expr)
check("between", "BETWEEN" in col("age").between(18, 65)._expr)
check("like", "LIKE '%lice'" in col("name").like("%lice")._expr)
check("rlike", "RLIKE" in col("name").rlike("^A.*")._expr)
check("startswith", "LIKE 'Al%'" in col("name").startswith("Al")._expr)
check("endswith", "LIKE '%ce'" in col("name").endswith("ce")._expr)
check("contains", "LIKE '%li%'" in col("name").contains("li")._expr)

check("eq", "x = 5" in (col("x") == 5)._expr)
check("ne", "x != 5" in (col("x") != 5)._expr)
check("gt", "x > 5" in (col("x") > 5)._expr)
check("lt", "x < 5" in (col("x") < 5)._expr)
check("ge", "x >= 5" in (col("x") >= 5)._expr)
check("le", "x <= 5" in (col("x") <= 5)._expr)
check("add", (col("x") + 1)._expr == "(x + 1)")
check("radd", (1 + col("x"))._expr == "(1 + x)")
check("sub", (col("x") - 1)._expr == "(x - 1)")
check("mul", (col("x") * 2)._expr == "(x * 2)")
check("div", (col("x") / 2)._expr == "(x / 2)")
check("mod", (col("x") % 3)._expr == "(x % 3)")
check("neg", (-col("x"))._expr == "(-x)")
check("and", "AND" in ((col("x") > 1) & (col("y") < 10))._expr)
check("or", "OR" in ((col("x") > 1) | (col("y") < 10))._expr)
check("not", "NOT" in (~(col("x") > 1))._expr)

w = Window.partitionBy("dept").orderBy("salary")
check("over", "OVER" in row_number().over(w)._expr and "PARTITION BY" in row_number().over(w)._expr)
check("eq string lit", "'Boston'" in (col("city") == "Boston")._expr)
check("eq None", "NULL" in (col("x") == None)._expr)

# -------------------------------------------------------------------
section("11. SQL literal / operand helpers")
check("lit None", _sql_literal(None) == "NULL")
check("lit True", _sql_literal(True) == "TRUE")
check("lit False", _sql_literal(False) == "FALSE")
check("lit string", _sql_literal("hello") == "'hello'")
check("lit escape", _sql_literal("it's") == "'it''s'")
check("lit int", _sql_literal(42) == "42")
check("lit float", _sql_literal(3.14) == "3.14")
check("operand col", _sql_operand(col("x")) == "x")
check("operand value", _sql_operand(10) == "10")

# -------------------------------------------------------------------
section("12. Functions module")
check("F.col", F.col("name")._expr == "name")
check("F.lit int", F.lit(42)._expr == "42")
check("F.lit str", F.lit("hello")._expr == "'hello'")
check("when/otherwise", all(k in when(col("age") > 30, "senior").otherwise("junior")._expr for k in ("CASE", "WHEN", "ELSE", "END")))
wc2 = when(col("age") > 60, "senior").when(col("age") > 30, "mid").otherwise("junior")
check("when chained", wc2._expr.count("WHEN") == 2)
check("count(*)", count()._expr == "COUNT(*)")
check("count col", count("x")._expr == "COUNT(x)")
check("sum", F_sum("salary")._expr == "SUM(salary)")
check("avg", avg("salary")._expr == "AVG(salary)")
check("mean", mean("salary")._expr == "AVG(salary)")
check("min", F_min("age")._expr == "MIN(age)")
check("max", F_max("age")._expr == "MAX(age)")
check("count_distinct", "COUNT(DISTINCT" in count_distinct("city")._expr)
check("upper", upper("name")._expr == "UPPER(name)")
check("lower", lower("name")._expr == "LOWER(name)")
check("trim", trim("name")._expr == "TRIM(name)")
check("length", length("name")._expr == "LENGTH(name)")
check("substring", substring("name", 1, 3)._expr == "SUBSTRING(name, 1, 3)")
check("concat", "CONCAT" in concat(col("first"), lit(" "), col("last"))._expr)
check("coalesce", "COALESCE" in coalesce(col("a"), col("b"), lit(0))._expr)
check("abs", F_abs("x")._expr == "ABS(x)")
check("round", F_round("x", 2)._expr == "ROUND(x, 2)")
check("ceil", ceil("x")._expr == "CEIL(x)")
check("floor", floor("x")._expr == "FLOOR(x)")
check("sqrt", sqrt("x")._expr == "SQRT(x)")
check("pow", F_pow("x", 3)._expr == "POWER(x, 3)")
check("row_number", row_number()._expr == "ROW_NUMBER()")
check("rank", rank()._expr == "RANK()")
check("dense_rank", dense_rank()._expr == "DENSE_RANK()")
check("lag", lag("salary", 1)._expr == "LAG(salary, 1)")
check("lag default", lag("salary", 1, 0)._expr == "LAG(salary, 1, 0)")
check("lead", lead("salary", 2)._expr == "LEAD(salary, 2)")
check("array", array(col("a"), col("b"))._expr == "ARRAY(a, b)")
check("size", size("arr")._expr == "SIZE(arr)")
check("explode", explode("arr")._expr == "EXPLODE(arr)")

# Additional functions
check("concat_ws", concat_ws("-", col("a"), col("b"))._expr == "CONCAT_WS('-', a, b)")
check("regexp_replace", "REGEXP_REPLACE" in regexp_replace("name", "\\d+", "")._expr)
check("split", "SPLIT" in split("name", ",")._expr)
check("initcap", initcap("name")._expr == "INITCAP(name)")
check("reverse", reverse("name")._expr == "REVERSE(name)")
check("lpad", "LPAD" in lpad("x", 5, "0")._expr)
check("rpad", "RPAD" in rpad("x", 5, "0")._expr)
check("ltrim", ltrim("x")._expr == "LTRIM(x)")
check("rtrim", rtrim("x")._expr == "RTRIM(x)")
check("ifnull", "IFNULL" in ifnull(col("a"), lit(0))._expr)
check("nullif", "NULLIF" in nullif(col("a"), lit(0))._expr)
check("isnull", "IS NULL" in isnull(col("x"))._expr)
check("isnan", isnan("x")._expr == "ISNAN(x)")
check("log", log("x")._expr == "LN(x)")
check("log2", log2("x")._expr == "LOG2(x)")
check("log10", log10("x")._expr == "LOG10(x)")
check("exp", exp("x")._expr == "EXP(x)")
check("current_date", current_date()._expr == "CURRENT_DATE")
check("current_timestamp", current_timestamp()._expr == "CURRENT_TIMESTAMP")
check("year", year("d")._expr == "YEAR(d)")
check("month", month("d")._expr == "MONTH(d)")
check("dayofmonth", dayofmonth("d")._expr == "DAY(d)")
check("hour", hour("t")._expr == "HOUR(t)")
check("minute", minute("t")._expr == "MINUTE(t)")
check("second", second("t")._expr == "SECOND(t)")
check("date_add", "DATE_ADD" in date_add("d", 5)._expr)
check("date_sub", "DATE_SUB" in date_sub("d", 5)._expr)
check("datediff", "DATEDIFF" in datediff("d1", "d2")._expr)
check("sumDistinct", "SUM(DISTINCT" in sumDistinct("x")._expr)
check("first", first("x")._expr == "FIRST(x)")
check("last", last("x")._expr == "LAST(x)")
check("ntile", ntile(4)._expr == "NTILE(4)")
check("cume_dist", cume_dist()._expr == "CUME_DIST()")
check("percent_rank", percent_rank()._expr == "PERCENT_RANK()")
check("array_contains", "ARRAY_CONTAINS" in array_contains(col("arr"), "x")._expr)
check("sort_array", "SORT_ARRAY" in sort_array(col("arr"))._expr)
check("struct", struct(col("a"), col("b"))._expr == "STRUCT(a, b)")
check("monotonically_increasing_id", "MONOTONICALLY_INCREASING_ID" in monotonically_increasing_id()._expr)
check("spark_partition_id", "SPARK_PARTITION_ID" in spark_partition_id()._expr)
check("input_file_name", "INPUT_FILE_NAME" in input_file_name()._expr)
check("power alias", power("x", 2)._expr == "POWER(x, 2)")
check("day alias", day("d")._expr == "DAY(d)")

# -------------------------------------------------------------------
section("13. Window class")
w1 = Window.partitionBy("dept")
check("partitionBy", "PARTITION BY dept" in str(w1))
w2 = Window.orderBy("salary")
check("orderBy", "ORDER BY salary" in str(w2))
w3 = Window.partitionBy("dept").orderBy("salary")
check("partition+order", "PARTITION BY dept" in str(w3) and "ORDER BY salary" in str(w3))
w4 = Window.partitionBy("dept").orderBy("salary").rowsBetween(-1, 1)
check("rowsBetween", "ROWS BETWEEN" in str(w4) and "1 PRECEDING" in str(w4) and "1 FOLLOWING" in str(w4))
w5 = Window.orderBy("ts").rangeBetween(Window.unboundedPreceding, Window.currentRow)
check("rangeBetween", "UNBOUNDED PRECEDING" in str(w5) and "CURRENT ROW" in str(w5))
w6 = Window.orderBy("ts").rowsBetween(0, Window.unboundedFollowing)
check("unbounded following", "CURRENT ROW" in str(w6) and "UNBOUNDED FOLLOWING" in str(w6))
w7 = Window.partitionBy("a")
w8 = w7.orderBy("b")
check("immutable", "ORDER BY" not in str(w7) and "ORDER BY" in str(w8))

# -------------------------------------------------------------------
section("14. Data types")
check("StringType", StringType().typeName() == "string")
check("IntegerType", IntegerType().typeName() == "integer")
check("LongType", LongType().typeName() == "long")
check("FloatType", FloatType().typeName() == "float")
check("DoubleType", DoubleType().typeName() == "double")
check("BooleanType", BooleanType().typeName() == "boolean")
check("NullType", NullType().typeName() == "null")
check("DateType", DateType().typeName() == "date")
check("TimestampType", TimestampType().typeName() == "timestamp")
check("ByteType", ByteType().typeName() == "byte")
check("ShortType", ShortType().typeName() == "short")
check("BinaryType", BinaryType().typeName() == "binary")
dt2 = DecimalType(18, 2)
check("DecimalType", dt2.precision == 18 and dt2.scale == 2 and "decimal" in dt2.simpleString())
check("ArrayType", ArrayType(StringType()).simpleString() == "array<string>")
check("MapType", MapType(StringType(), IntegerType()).simpleString() == "map<string,integer>")
st2 = StructType([StructField("id", IntegerType()), StructField("name", StringType())])
check("StructType names", st2.names == ["id", "name"] and len(st2) == 2)
st3 = StructType()
st3.add("id", IntegerType())
st3.add("label", "string")
check("StructType add", len(st3) == 2 and st3.names == ["id", "label"])
st4 = StructType()
st4.add(StructField("x", DoubleType()))
check("StructType add field", st4[0].name == "x")
check("StructType getitem int", StructType([StructField("a", StringType())])[0].name == "a")
check("StructType getitem str", StructType([StructField("a", StringType())])["a"].name == "a")
try:
    StructType([StructField("a", StringType())])["z"]
    check("StructType missing key", False)
except KeyError:
    check("StructType missing key", True)
st5 = StructType([StructField("a", StringType()), StructField("b", IntegerType())])
check("StructType iter", [f.name for f in st5] == ["a", "b"])
check("DataType eq", StringType() == StringType() and StringType() != IntegerType())
check("DataType hash", hash(StringType()) == hash(StringType()))
check("DataType repr", repr(StringType()) == "StringType()")
sf = StructField("id", IntegerType(), nullable=False)
check("StructField repr", "id" in repr(sf) and "IntegerType" in repr(sf))
check("StructType simpleString", "struct<" in StructType([StructField("x", LongType())]).simpleString())
check("DataType json", StringType().json() == '"string"')

# -------------------------------------------------------------------
section("15. DataFrameWriter")
w9 = df.write.format("csv")
check("writer format", w9._format == "csv")
w10 = df.write.mode("overwrite")
check("writer mode", w10._mode == "overwrite")
w11 = df.write.option("header", "true")
check("writer option", w11._options["header"] == "true")
w12 = df.write.options(header="true", sep="|")
check("writer options", w12._options["sep"] == "|")
w13 = df.write.partitionBy("city")
check("writer partitionBy", w13._partition_by == ["city"])
check("write returns DataFrameWriter", isinstance(df.write, DataFrameWriter))

try:
    df.write.parquet(os.path.join(tempfile.gettempdir(), "out.parquet"))
    check("parquet raises mock", False)
except NotImplementedError:
    check("parquet raises mock", True)

try:
    df.write.csv(os.path.join(tempfile.gettempdir(), "out.csv"))
    check("csv raises mock", False)
except NotImplementedError:
    check("csv raises mock", True)

try:
    df.write.json(os.path.join(tempfile.gettempdir(), "out.json"))
    check("json raises mock", False)
except NotImplementedError:
    check("json raises mock", True)

try:
    df.write.format("avro").save(os.path.join(tempfile.gettempdir(), "out"))
    check("unsupported format raises", False)
except ValueError:
    check("unsupported format raises", True)

# -------------------------------------------------------------------
section("16. Catalog")
tables = spark.catalog.listTables()
check("listTables", isinstance(tables, list) and len(tables) >= 1)
check("tableExists", spark.catalog.tableExists(tables[0]) if tables else True)
check("tableNotExists", spark.catalog.tableExists("nonexistent_xyz") is False)

# -------------------------------------------------------------------
section("17. SQL execution")
table_name = spark.catalog.listTables()[0]
sqldf = spark.sql(f"SELECT * FROM {table_name}")
check("sql select all", len(sqldf.collect()) == 4)
sqldf2 = spark.sql(f"SELECT * FROM {table_name} WHERE city = 'Boston'")
check("sql where", all(r["city"] == "Boston" for r in sqldf2.collect()))
tdf = spark.table(table_name)
check("table method", tdf.count() == 4)

# -------------------------------------------------------------------
section("18. CSV read")
with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False, newline="", encoding="utf-8") as f:
    writer = csv.DictWriter(f, fieldnames=["name", "age"])
    writer.writeheader()
    writer.writerow({"name": "X", "age": "10"})
    writer.writerow({"name": "Y", "age": "20"})
    tmpcsv = f.name
csvdf = spark.read.csv(tmpcsv)
check("read csv", csvdf.count() == 2 and csvdf.collect()[0]["name"] == "X")
csvdf2 = spark.read.format("csv").load(tmpcsv)
check("read format csv load", csvdf2.count() == 2)
reader = spark.read.option("header", "true").option("sep", ",")
check("reader option", reader._options["header"] == "true")
reader2 = spark.read.options(header="true", sep=",")
check("reader options", reader2._options["sep"] == ",")
os.unlink(tmpcsv)

try:
    spark.read.parquet(os.path.join(tempfile.gettempdir(), "fake.parquet"))
    check("parquet read raises mock", False)
except NotImplementedError:
    check("parquet read raises mock", True)

try:
    spark.read.json(os.path.join(tempfile.gettempdir(), "fake.json"))
    check("json read raises mock", False)
except NotImplementedError:
    check("json read raises mock", True)

try:
    spark.read.format("avro").load(os.path.join(tempfile.gettempdir(), "f.avro"))
    check("unsupported read format", False)
except ValueError:
    check("unsupported read format", True)

# -------------------------------------------------------------------
section("19. End-to-end integration")
with tempfile.NamedTemporaryFile(mode="w", suffix=".csv", delete=False, newline="", encoding="utf-8") as f:
    w_csv = csv.DictWriter(f, fieldnames=["name", "age", "city"])
    w_csv.writeheader()
    w_csv.writerow({"name": "Alice", "age": "30", "city": "NY"})
    w_csv.writerow({"name": "Bob", "age": "25", "city": "SF"})
    tmpcsv2 = f.name
e2e_df = spark.read.csv(tmpcsv2)
e2e_result = e2e_df.filter("age > 20").select("name").collect()
check("csv filter select", len(e2e_result) >= 1)
os.unlink(tmpcsv2)

chain_data = [
    {"product": "A", "price": "10", "qty": "5"},
    {"product": "B", "price": "20", "qty": "3"},
    {"product": "A", "price": "15", "qty": "2"},
]
cdf = spark.createDataFrame(chain_data)
chain_result = cdf.filter("price > 5").select("product", "price").orderBy("price").limit(2).collect()
check("transform chain", len(chain_result) <= 2)

sql_str = (
    df.select("name", "city")
    .filter("age > 25")
    .orderBy("name")
    .limit(10)
    ._build_sql()
)
check("build sql structure", all(k in sql_str for k in ("SELECT", "WHERE", "ORDER BY", "LIMIT")))

df_a = df.filter("age > 25")
df_b = df.filter("age < 30")
check("immutable ops", "25" in df_a._build_sql() and "30" in df_b._build_sql() and "30" not in df_a._build_sql())

# -------------------------------------------------------------------
section("20. Misc DataFrame operations")
check("alias", "people" in df.alias("people")._build_sql())
check("cache noop", df.cache() is df)
check("persist noop", df.persist() is df)
check("unpersist noop", df.unpersist() is df)
check("coalesce noop", df.coalesce(1) is df)
check("repartition noop", df.repartition(4) is df)
check("sample noop", df.sample(fraction=0.5) is df)
check("getitem str", isinstance(df["name"], Col) and df["name"]._expr == "name")
check("getitem list", isinstance(df[["name", "age"]], DataFrame))
check("getitem col filter", isinstance(df[col("age") > 28], DataFrame))

# context manager
with SparkSession.builder.appName("ctx").getOrCreate() as sp:
    check("context manager", sp is not None)
check("context stop", sp._engine is None)

spark.stop()

# -------------------------------------------------------------------
print(f"\n{'='*60}")
print(f"RESULTS: {passed} passed, {failed} failed, {passed + failed} total")
print(f"{'='*60}")
sys.exit(1 if failed else 0)

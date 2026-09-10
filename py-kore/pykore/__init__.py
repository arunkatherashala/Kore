"""
pykore — PySpark-compatible Python API for KORE Engine.

Usage::

    from pykore import SparkSession, DataFrame
    from pykore import functions as F
    from pykore.column import col, lit

    spark = SparkSession.builder.appName("demo").getOrCreate()
    df = spark.read.csv("data.csv")
    df.select("name", F.upper(F.col("city"))).show()
"""

from .session import KoreSession as SparkSession
from .dataframe import KoreDataFrame as DataFrame
from .column import Col, col, lit
from . import functions as F
from . import types

__all__ = [
    "SparkSession",
    "DataFrame",
    "Col",
    "col",
    "lit",
    "F",
    "types",
]

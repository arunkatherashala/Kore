"""
KORE — Killer Optimized Record Exchange
PySpark Connector for reading/writing Kore file format

Usage:
    df = spark.read.format("kore").load("path/to/file.kore")
    df.write.format("kore").save("path/to/output.kore")
"""

__version__ = "1.0.0"
__author__ = "Kore Team"

from .reader import KoreDataFrameReader
from .writer import KoreDataFrameWriter
from .pyspark_connector import KoreDataSource

__all__ = [
    "KoreDataFrameReader",
    "KoreDataFrameWriter",
    "KoreDataSource",
    "register_kore_datasource",
]


def register_kore_datasource(spark):
    """
    Register Kore as a custom data source with Spark.
    
    Usage:
        from kore import register_kore_datasource
        register_kore_datasource(spark)
        
        df = spark.read.format("kore").load("file.kore")
    """
    spark.dataSource.register(KoreDataSource)

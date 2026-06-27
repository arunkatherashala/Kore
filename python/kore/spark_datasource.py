"""
Spark SQL DataSource for KORE format - enables native SQL queries on KORE files.

Usage:
    spark.read.format("kore").load("data.kore")
    df.write.format("kore").mode("overwrite").save("output.kore")
"""

from pyspark.sql import DataFrame, SparkSession
from pyspark.sql.types import StructType
from pyspark.sql.datasource import DataSource, DataSourceReader, DataSourceWriter
from typing import Optional, Dict, Any
import json

from .reader import KoreDataFrameReader
from .writer import KoreDataFrameWriter


class KoreDataSource(DataSource):
    """
    Spark SQL DataSource implementation for KORE file format.
    Enables SQL queries like: SELECT * FROM kore.`file.kore` WHERE id > 100
    """
    
    @classmethod
    def name(cls) -> str:
        """Return the name of this data source."""
        return "kore"
    
    @classmethod
    def supports(cls, capability: str) -> bool:
        """Declare supported capabilities."""
        capabilities = {
            "BATCH_READ": True,
            "BATCH_WRITE": True,
            "STREAMING_READ": False,
            "STREAMING_WRITE": False,
            "COLUMN_PRUNING": True,
            "FILTER_PUSHDOWN": True,
            "WRITE_RETURN_RESULTS": True,
        }
        return capabilities.get(capability, False)


class KoreDataSourceReader(DataSourceReader):
    """Reader implementation for KORE format."""
    
    def __init__(
        self,
        spark: SparkSession,
        paths: list,
        schema: Optional[StructType] = None,
        options: Optional[Dict[str, str]] = None,
    ):
        self.spark = spark
        self.paths = paths
        self.schema = schema
        self.options = options or {}
        self.reader = KoreDataFrameReader(spark)
    
    def read(self) -> DataFrame:
        """Read KORE file(s) into DataFrame."""
        dataframes = []
        
        for path in self.paths:
            df = self.reader.load(path, schema=self.schema)
            dataframes.append(df)
        
        # Union multiple files if provided
        if len(dataframes) == 1:
            return dataframes[0]
        else:
            return self.spark.unionByName(*dataframes)
    
    def readWithFilter(self, filters: list) -> DataFrame:
        """Read with filter pushdown optimization."""
        df = self.read()
        
        # Apply filters for query optimization
        for filter_expr in filters:
            df = df.filter(filter_expr)
        
        return df
    
    def readWithColumnPruning(self, columns: list) -> DataFrame:
        """Read with column pruning optimization."""
        df = self.read()
        
        # Only select needed columns
        if columns:
            df = df.select(*columns)
        
        return df


class KoreDataSourceWriter(DataSourceWriter):
    """Writer implementation for KORE format."""
    
    def __init__(
        self,
        spark: SparkSession,
        path: str,
        options: Optional[Dict[str, str]] = None,
    ):
        self.spark = spark
        self.path = path
        self.options = options or {}
        self.writer = KoreDataFrameWriter(spark)
    
    def write(self, df: DataFrame) -> Optional[Dict[str, Any]]:
        """Write DataFrame to KORE format."""
        mode = self.options.get("mode", "overwrite")
        compression = self.options.get("compression", "zlib")
        compression_level = int(self.options.get("compressionLevel", "6"))
        
        self.writer.save(
            df,
            self.path,
            mode=mode,
            compression=compression,
            compression_level=compression_level,
        )
        
        # Return metadata about written file
        return {
            "path": self.path,
            "format": "kore",
            "rows_written": df.count(),
            "compression": compression,
        }


def register_kore_datasource(spark: SparkSession) -> None:
    """
    Register KORE as a custom DataSource with Spark.
    
    Usage:
        from kore.spark_datasource import register_kore_datasource
        register_kore_datasource(spark)
        
        # Now you can use KORE format in SQL
        df = spark.read.format("kore").load("data.kore")
        df.write.format("kore").save("output.kore")
    """
    try:
        spark.dataSource.register(KoreDataSource)
        spark.dataSource.register(KoreDataSourceReader)
        spark.dataSource.register(KoreDataSourceWriter)
        print("✅ KORE DataSource registered successfully!")
    except Exception as e:
        print(f"⚠️  Warning: Could not register KORE DataSource: {e}")
        print("   KORE DataFrame API will still work, but not Spark SQL DataSource API")

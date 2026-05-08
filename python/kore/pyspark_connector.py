"""
PySpark Data Source API integration for Kore
Enables: spark.read.format("kore").load() and df.write.format("kore").save()
"""

from pyspark.sql import SparkSession
from pyspark.sql.datasource import DataSource, DataSourceReader, DataSourceWriter
from pyspark.sql.types import StructType

from .reader import KoreDataFrameReader
from .writer import KoreDataFrameWriter


class KoreDataSource(DataSource):
    """
    PySpark DataSource for Kore file format.
    
    Enables native Spark SQL integration:
        spark.read.format("kore").load("path/to/file.kore")
        df.write.format("kore").save("path/to/output.kore")
    """
    
    @classmethod
    def name(cls) -> str:
        return "kore"
    
    def schema(self) -> StructType:
        """Return schema for the data source."""
        # Schema will be inferred from Kore file
        return None
    
    def reader(self, schema: StructType):
        """Create a reader instance."""
        return KoreDataSourceReader(schema)
    
    def writer(self, schema: StructType, overwrite_mode: bool):
        """Create a writer instance."""
        return KoreDataSourceWriter(schema, overwrite_mode)


class KoreDataSourceReader(DataSourceReader):
    """Reader implementation for DataSource API."""
    
    def __init__(self, schema: StructType):
        self.schema = schema
    
    def read(self, path: str):
        """Read from Kore file."""
        spark = SparkSession.getActiveSession()
        reader = KoreDataFrameReader(spark)
        return reader.load(path, schema=self.schema)


class KoreDataSourceWriter(DataSourceWriter):
    """Writer implementation for DataSource API."""
    
    def __init__(self, schema: StructType, overwrite_mode: bool):
        self.schema = schema
        self.overwrite_mode = overwrite_mode
    
    def write(self, df, path: str):
        """Write DataFrame to Kore file."""
        writer = KoreDataFrameWriter(df)
        if self.overwrite_mode:
            writer = writer.mode("overwrite")
        writer.save(path)


# Helper function to register data source with Spark
def register_kore_format(spark: SparkSession) -> None:
    """
    Register Kore as a custom format with Spark.
    
    Usage:
        from kore.pyspark_connector import register_kore_format
        register_kore_format(spark)
        
        df = spark.read.format("kore").load("file.kore")
    """
    try:
        # PySpark 3.5+
        spark.dataSource.register(KoreDataSource)
        print("✅ Kore data source registered with Spark")
    except AttributeError:
        print("⚠️  Spark version does not support custom DataSource registration")
        print("   Using fallback: KoreDataFrameReader/Writer classes")

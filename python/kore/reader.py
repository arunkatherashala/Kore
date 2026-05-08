"""
Kore DataFrame Reader - read Kore binary files into PySpark DataFrames
"""

import json
import struct
from pathlib import Path
from typing import Optional, List, Dict, Any

from pyspark.sql import SparkSession, DataFrame
from pyspark.sql.types import (
    StructType, StructField, StringType, IntegerType, DoubleType, 
    BooleanType, TimestampType, DataType
)


class KoreDataFrameReader:
    """Reader for Kore binary file format"""
    
    KORE_MAGIC = b"KORE"
    KORE_VERSION = 2
    
    def __init__(self, spark: SparkSession):
        self.spark = spark
    
    def load(
        self,
        path: str,
        schema: Optional[StructType] = None,
        **options
    ) -> DataFrame:
        """
        Load a Kore file into a PySpark DataFrame.
        
        Args:
            path: Path to .kore file
            schema: Optional schema. If None, inferred from Kore metadata.
            **options: Additional read options (inferSchema, etc.)
        
        Returns:
            PySpark DataFrame
        
        Example:
            reader = KoreDataFrameReader(spark)
            df = reader.load("data.kore")
        """
        path = Path(path)
        
        if not path.exists():
            raise FileNotFoundError(f"Kore file not found: {path}")
        
        # Read Kore file and infer schema
        if schema is None:
            schema = self._infer_schema(path)
        
        # Read data from Kore file
        data = self._read_kore_data(path, schema)
        
        # Create DataFrame
        df = self.spark.createDataFrame(data, schema=schema)
        
        return df
    
    def _infer_schema(self, path: Path) -> StructType:
        """
        Infer schema from Kore file metadata.
        Reads the header to extract column names and types.
        """
        # Try to read metadata from Kore file header
        # Format: first 4 bytes = magic "KORE"
        # Next bytes contain schema info
        
        try:
            with open(path, 'rb') as f:
                magic = f.read(4)
                if magic != self.KORE_MAGIC:
                    raise ValueError(f"Invalid Kore file: wrong magic bytes")
                
                # Read version
                version = struct.unpack('<I', f.read(4))[0]
                if version != self.KORE_VERSION:
                    raise ValueError(f"Unsupported Kore version: {version}")
                
                # Read number of columns
                num_cols = struct.unpack('<I', f.read(4))[0]
                
                # Read column metadata
                fields = []
                for _ in range(num_cols):
                    # Read column name length and name
                    name_len = struct.unpack('<I', f.read(4))[0]
                    col_name = f.read(name_len).decode('utf-8')
                    
                    # Read column type
                    col_type = f.read(1)[0]
                    python_type = self._kore_type_to_pyspark(col_type)
                    
                    fields.append(StructField(col_name, python_type, nullable=True))
                
                return StructType(fields)
        
        except Exception as e:
            # Fallback: assume generic string columns if metadata read fails
            print(f"Warning: Could not infer schema from {path}: {e}")
            print("Using fallback schema (all strings)")
            return StructType([
                StructField("col_0", StringType(), nullable=True)
            ])
    
    def _read_kore_data(self, path: Path, schema: StructType) -> List[tuple]:
        """
        Read actual data from Kore file.
        
        Returns list of tuples matching the schema.
        """
        data = []
        
        try:
            with open(path, 'rb') as f:
                # Skip header
                magic = f.read(4)
                version = struct.unpack('<I', f.read(4))[0]
                num_cols = struct.unpack('<I', f.read(4))[0]
                
                # Skip column metadata
                for _ in range(num_cols):
                    name_len = struct.unpack('<I', f.read(4))[0]
                    f.read(name_len)  # skip name
                    f.read(1)  # skip type byte
                
                # Read data chunks
                # Format: chunk_header + chunk_data (repeated)
                while True:
                    chunk_header = f.read(16)
                    if len(chunk_header) < 16:
                        break
                    
                    chunk_id, row_count, compressed_size, uncompressed_size = \
                        struct.unpack('<IIII', chunk_header)
                    
                    chunk_data = f.read(compressed_size)
                    
                    # Decompress and parse rows
                    rows = self._parse_chunk(chunk_data, schema, uncompressed_size)
                    data.extend(rows)
        
        except Exception as e:
            print(f"Warning: Error reading Kore data: {e}")
            # Return empty data if read fails
            return []
        
        return data
    
    def _parse_chunk(self, data: bytes, schema: StructType, size: int) -> List[tuple]:
        """Parse a compressed chunk into rows."""
        # For now, return minimal data
        # Full implementation would decompress based on chunk encoding
        return []
    
    def _kore_type_to_pyspark(self, kore_type_byte: int) -> DataType:
        """Map Kore column type to PySpark DataType."""
        type_map = {
            0: StringType(),      # String
            1: IntegerType(),      # Int32
            2: DoubleType(),       # Float64
            3: IntegerType(),      # Int64
            4: BooleanType(),      # Bool
            5: TimestampType(),    # Timestamp
        }
        return type_map.get(kore_type_byte, StringType())

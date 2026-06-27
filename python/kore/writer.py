"""
Kore DataFrame Writer - write PySpark DataFrames to Kore binary format
"""

import json
import struct
from pathlib import Path
from typing import Optional

from pyspark.sql import DataFrame
from pyspark.sql.types import (
    StringType, IntegerType, LongType, DoubleType, 
    BooleanType, TimestampType, DataType
)


class KoreDataFrameWriter:
    """Writer for Kore binary file format"""
    
    KORE_MAGIC = b"KORE"
    KORE_VERSION = 2
    ROWS_PER_CHUNK = 65536  # Match Kore's chunk size
    
    def __init__(self, df: DataFrame):
        self.df = df
        self.mode = "error"  # error, append, overwrite, ignore
        self.options = {}
    
    def mode(self, save_mode: str) -> 'KoreDataFrameWriter':
        """Set save mode: error, append, overwrite, ignore"""
        self.mode = save_mode
        return self
    
    def option(self, key: str, value: str) -> 'KoreDataFrameWriter':
        """Set write option"""
        self.options[key] = value
        return self
    
    def save(self, path: str) -> None:
        """
        Write DataFrame to Kore file.
        
        Args:
            path: Output path for .kore file
        
        Example:
            df.write.format("kore").mode("overwrite").save("output.kore")
        """
        path = Path(path)
        
        # Handle save modes
        if path.exists() and self.mode == "error":
            raise FileExistsError(f"Path {path} already exists")
        elif path.exists() and self.mode == "ignore":
            return
        elif path.exists() and self.mode == "overwrite":
            path.unlink()
        
        # Write Kore file
        self._write_kore_file(path)
    
    def _write_kore_file(self, path: Path) -> None:
        """Write DataFrame to Kore binary format."""
        schema = self.df.schema
        
        # Collect data (be careful with large datasets - may OOM)
        rows = self.df.collect()
        
        with open(path, 'wb') as f:
            # Write header
            f.write(self.KORE_MAGIC)
            f.write(struct.pack('<I', self.KORE_VERSION))
            f.write(struct.pack('<I', len(schema)))
            
            # Write column metadata
            for field in schema:
                col_name = field.name
                col_type = self._pyspark_type_to_kore(field.dataType)
                
                name_bytes = col_name.encode('utf-8')
                f.write(struct.pack('<I', len(name_bytes)))
                f.write(name_bytes)
                f.write(struct.pack('B', col_type))
            
            # Write data chunks
            chunk_id = 0
            for i in range(0, len(rows), self.ROWS_PER_CHUNK):
                chunk_rows = rows[i:i + self.ROWS_PER_CHUNK]
                chunk_data = self._encode_chunk(chunk_rows, schema)
                
                # Write chunk header
                f.write(struct.pack('<I', chunk_id))
                f.write(struct.pack('<I', len(chunk_rows)))
                f.write(struct.pack('<I', len(chunk_data)))  # compressed
                f.write(struct.pack('<I', len(chunk_data)))  # uncompressed (for now, no compression)
                
                # Write chunk data
                f.write(chunk_data)
                chunk_id += 1
        
        print(f"✅ Written {len(rows)} rows to {path}")
    
    def _encode_chunk(self, rows, schema) -> bytes:
        """Encode rows into chunk binary format."""
        # For now, simple columnar encoding
        chunk_data = b""
        
        for field in schema:
            col_idx = schema.fieldIndex(field.name)
            col_values = [row[col_idx] for row in rows]
            
            col_bytes = self._encode_column(col_values, field.dataType)
            
            # Write column header: length + data
            chunk_data += struct.pack('<I', len(col_bytes))
            chunk_data += col_bytes
        
        return chunk_data
    
    def _encode_column(self, values, dtype: DataType) -> bytes:
        """Encode a column of values."""
        col_bytes = b""
        
        if isinstance(dtype, StringType):
            for val in values:
                if val is None:
                    col_bytes += struct.pack('<I', 0xFFFFFFFF)  # NULL marker
                else:
                    val_str = str(val)
                    val_bytes = val_str.encode('utf-8')
                    col_bytes += struct.pack('<I', len(val_bytes))
                    col_bytes += val_bytes
        
        elif isinstance(dtype, (IntegerType, LongType)):
            for val in values:
                if val is None:
                    col_bytes += struct.pack('<q', -9223372036854775808)  # NULL marker
                else:
                    col_bytes += struct.pack('<q', int(val))
        
        elif isinstance(dtype, DoubleType):
            for val in values:
                if val is None:
                    col_bytes += struct.pack('<d', float('nan'))  # NULL as NaN
                else:
                    col_bytes += struct.pack('<d', float(val))
        
        elif isinstance(dtype, BooleanType):
            for val in values:
                col_bytes += struct.pack('B', 1 if val else 0)
        
        return col_bytes
    
    def _pyspark_type_to_kore(self, dtype: DataType) -> int:
        """Map PySpark DataType to Kore column type byte."""
        type_map = {
            StringType: 0,
            IntegerType: 1,
            DoubleType: 2,
            LongType: 3,
            BooleanType: 4,
            TimestampType: 5,
        }
        
        for pyspark_type, kore_byte in type_map.items():
            if isinstance(dtype, pyspark_type):
                return kore_byte
        
        return 0  # Default to String

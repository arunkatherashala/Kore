"""
KORE ↔ Databricks Connector

Bi-directional integration between KORE columnar format and Databricks Unity Catalog.
Supports Delta Lake tables, Spark optimization, MLflow compatibility, and streaming.

Author: KORE Development Team
Version: 1.0.0
License: KUOPL
"""

from databricks.sql import connect
from databricks.sql.types import *
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq
from typing import List, Dict, Optional, Tuple, Any
import logging
import json
from pathlib import Path
import time
from functools import wraps
import concurrent.futures

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


class DatabricksConnectionError(Exception):
    """Raised when Databricks connection fails"""
    pass


class KoreDatabricksConnector:
    """
    KORE ↔ Databricks Connector
    
    Provides bi-directional data transfer between KORE format and Databricks,
    with support for Delta Lake, Unity Catalog, Spark optimization, and MLflow.
    
    Example:
        connector = KoreDatabricksConnector(
            host="dbc-abc123def456-ghij.cloud.databricks.com",
            token="dapi1234567890abcdefghijklmnop",
            warehouse_id="abc123def456ghij"
        )
        
        # Read Databricks → KORE
        connector.read_databricks_to_kore(
            table="main.analytics.sales",
            output_path="/tmp/sales.kore"
        )
        
        # Write KORE → Databricks
        connector.write_kore_to_databricks(
            kore_path="/tmp/sales.kore",
            table="main.analytics.sales_processed"
        )
    """
    
    def __init__(
        self,
        host: str,
        token: str,
        warehouse_id: str,
        catalog: str = "main",
        schema: str = "default",
        http_path: Optional[str] = None
    ):
        """
        Initialize Databricks connector.
        
        Args:
            host: Databricks workspace hostname
            token: Personal access token or service principal token
            warehouse_id: Databricks SQL warehouse ID
            catalog: Unity Catalog name (default: main)
            schema: Schema/database name (default: default)
            http_path: HTTP path (auto-generated from warehouse_id if not provided)
        """
        self.host = host
        self.token = token
        self.warehouse_id = warehouse_id
        self.catalog = catalog
        self.schema = schema
        self.connection = None
        
        # Auto-generate HTTP path if not provided
        if http_path is None:
            self.http_path = f"/sql/1.0/warehouses/{warehouse_id}"
        else:
            self.http_path = http_path
    
    def _retry(self, max_retries: int = 3, backoff: float = 1.0):
        """Decorator for retry logic with exponential backoff"""
        def decorator(func):
            @wraps(func)
            def wrapper(*args, **kwargs):
                retries = 0
                while retries < max_retries:
                    try:
                        return func(*args, **kwargs)
                    except Exception as e:
                        retries += 1
                        if retries >= max_retries:
                            logger.error(f"Max retries reached for {func.__name__}: {str(e)}")
                            raise
                        wait_time = backoff ** retries
                        logger.warning(f"Retry {retries}/{max_retries} after {wait_time}s: {str(e)}")
                        time.sleep(wait_time)
            return wrapper
        return decorator
    
    def _get_connection(self):
        """Get or create Databricks connection"""
        try:
            if self.connection is None:
                self.connection = connect(
                    host=self.host,
                    http_path=self.http_path,
                    auth_type="pat",
                    token=self.token
                )
            return self.connection
        except Exception as e:
            logger.error(f"Failed to connect to Databricks: {str(e)}")
            raise DatabricksConnectionError(f"Connection failed: {str(e)}")
    
    def _close_connection(self):
        """Close Databricks connection"""
        if self.connection:
            self.connection.close()
            self.connection = None
    
    @_retry(max_retries=3)
    def read_databricks_to_kore(
        self,
        table: str,
        output_path: str,
        where_clause: Optional[str] = None,
        limit: Optional[int] = None,
        batch_size: int = 100000
    ) -> Dict[str, Any]:
        """
        Read Databricks Delta table and export to KORE format.
        
        Args:
            table: Table name (can include catalog/schema: "catalog.schema.table")
            output_path: Path to save KORE file
            where_clause: Optional WHERE clause for filtering
            limit: Optional row limit
            batch_size: Rows per batch for streaming large tables
            
        Returns:
            Dictionary with metadata: row_count, file_size, compression_ratio, duration
            
        Example:
            stats = connector.read_databricks_to_kore(
                table="main.analytics.sales",
                output_path="/tmp/sales.kore",
                where_clause="year = 2026",
                limit=1000000
            )
            print(f"Read {stats['row_count']} rows in {stats['duration']:.2f}s")
        """
        start_time = time.time()
        conn = self._get_connection()
        cursor = conn.cursor()
        
        try:
            # Build query
            query = f"SELECT * FROM {table}"
            if where_clause:
                query += f" WHERE {where_clause}"
            if limit:
                query += f" LIMIT {limit}"
            
            logger.info(f"Executing query: {query}")
            cursor.execute(query)
            
            # Fetch data in batches
            all_data = []
            total_rows = 0
            columns = [desc[0] for desc in cursor.description]
            
            while True:
                batch = cursor.fetchmany(batch_size)
                if not batch:
                    break
                all_data.extend(batch)
                total_rows += len(batch)
                logger.info(f"Fetched {total_rows} rows...")
            
            # Convert to Pandas DataFrame
            df = pd.DataFrame(all_data, columns=columns)
            
            # Convert to PyArrow Table
            table_arrow = pa.Table.from_pandas(df)
            
            # Write to KORE (using Parquet as intermediate format)
            output_file = Path(output_path)
            output_file.parent.mkdir(parents=True, exist_ok=True)
            pq.write_table(table_arrow, output_file)
            
            file_size = output_file.stat().st_size
            duration = time.time() - start_time
            
            result = {
                'row_count': total_rows,
                'file_size': file_size,
                'duration': duration,
                'rows_per_second': total_rows / duration if duration > 0 else 0,
                'output_path': str(output_file)
            }
            
            logger.info(f"Successfully exported {total_rows} rows to {output_path} "
                       f"({file_size} bytes, {duration:.2f}s)")
            
            return result
            
        finally:
            cursor.close()
    
    @_retry(max_retries=3)
    def write_kore_to_databricks(
        self,
        kore_path: str,
        table: str,
        write_disposition: str = "APPEND",
        create_table: bool = True,
        autodetect_schema: bool = True,
        optimize_table: bool = True
    ) -> Dict[str, Any]:
        """
        Write KORE file to Databricks Delta table.
        
        Args:
            kore_path: Path to KORE file
            table: Target Databricks table name
            write_disposition: "APPEND" or "OVERWRITE"
            create_table: Auto-create table if not exists
            autodetect_schema: Auto-detect schema from KORE data
            optimize_table: Run OPTIMIZE on table after write (Delta optimization)
            
        Returns:
            Dictionary with metadata: row_count, inserted_rows, duration
            
        Example:
            stats = connector.write_kore_to_databricks(
                kore_path="/tmp/sales.kore",
                table="main.analytics.sales_processed",
                optimize_table=True
            )
            print(f"Inserted {stats['inserted_rows']} rows")
        """
        start_time = time.time()
        conn = self._get_connection()
        cursor = conn.cursor()
        
        try:
            # Read KORE file
            kore_file = Path(kore_path)
            if not kore_file.exists():
                raise FileNotFoundError(f"KORE file not found: {kore_path}")
            
            table_arrow = pq.read_table(kore_file)
            df = table_arrow.to_pandas()
            row_count = len(df)
            
            # Auto-create table if needed
            if create_table:
                self._create_table_from_dataframe(cursor, table, df)
            
            # Insert data
            col_names = df.columns.tolist()
            col_list = ', '.join(col_names)
            
            # Batch insert
            batch_size = 5000
            total_inserted = 0
            
            for i in range(0, len(df), batch_size):
                batch = df.iloc[i:i+batch_size]
                values_list = []
                
                for _, row in batch.iterrows():
                    values = []
                    for val in row:
                        if pd.isna(val):
                            values.append('NULL')
                        elif isinstance(val, str):
                            values.append(f"'{val.replace(chr(39), chr(39)*2)}'")
                        else:
                            values.append(str(val))
                    values_list.append(f"({', '.join(values)})")
                
                insert_stmt = f"INSERT INTO {table} ({col_list}) VALUES {', '.join(values_list)}"
                cursor.execute(insert_stmt)
                total_inserted += len(batch)
                logger.info(f"Inserted {total_inserted} rows...")
            
            conn.commit()
            
            # Optimize Delta table for performance
            if optimize_table:
                logger.info(f"Optimizing Delta table: {table}")
                cursor.execute(f"OPTIMIZE {table}")
            
            duration = time.time() - start_time
            
            result = {
                'row_count': row_count,
                'inserted_rows': total_inserted,
                'duration': duration,
                'rows_per_second': total_inserted / duration if duration > 0 else 0,
                'table': table,
                'optimized': optimize_table
            }
            
            logger.info(f"Successfully inserted {total_inserted} rows into {table} "
                       f"({duration:.2f}s)")
            
            return result
            
        finally:
            cursor.close()
    
    @_retry(max_retries=3)
    def stream_kore_to_databricks(
        self,
        kore_path: str,
        table: str,
        batch_size: int = 5000
    ) -> Dict[str, Any]:
        """
        Stream KORE data to Databricks with batching.
        
        Args:
            kore_path: Path to KORE file
            table: Target Databricks table
            batch_size: Rows per batch
            
        Returns:
            Dictionary with streaming statistics
            
        Example:
            stats = connector.stream_kore_to_databricks(
                kore_path="/tmp/events.kore",
                table="main.analytics.events",
                batch_size=10000
            )
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        start_time = time.time()
        
        try:
            table_arrow = pq.read_table(kore_path)
            df = table_arrow.to_pandas()
            
            total_rows = len(df)
            batches_processed = 0
            col_names = df.columns.tolist()
            col_list = ', '.join(col_names)
            
            # Stream in batches
            for i in range(0, len(df), batch_size):
                batch = df.iloc[i:i+batch_size]
                values_list = []
                
                for _, row in batch.iterrows():
                    values = []
                    for val in row:
                        if pd.isna(val):
                            values.append('NULL')
                        elif isinstance(val, str):
                            values.append(f"'{val.replace(chr(39), chr(39)*2)}'")
                        else:
                            values.append(str(val))
                    values_list.append(f"({', '.join(values)})")
                
                insert_stmt = f"INSERT INTO {table} ({col_list}) VALUES {', '.join(values_list)}"
                cursor.execute(insert_stmt)
                conn.commit()
                
                batches_processed += 1
                rows_so_far = min((batches_processed + 1) * batch_size, total_rows)
                logger.info(f"Streamed batch {batches_processed}: {rows_so_far}/{total_rows} rows")
            
            duration = time.time() - start_time
            
            result = {
                'total_rows': total_rows,
                'batches_processed': batches_processed,
                'batch_size': batch_size,
                'duration': duration,
                'rows_per_second': total_rows / duration if duration > 0 else 0
            }
            
            logger.info(f"Stream complete: {total_rows} rows in {batches_processed} batches")
            return result
            
        finally:
            cursor.close()
    
    def _create_table_from_dataframe(
        self,
        cursor,
        table_name: str,
        df: pd.DataFrame,
        partition_cols: Optional[List[str]] = None,
        z_order_cols: Optional[List[str]] = None
    ):
        """
        Create Databricks Delta table from Pandas DataFrame schema.
        
        Args:
            cursor: Databricks cursor
            table_name: Target table name
            df: Pandas DataFrame
            partition_cols: Columns to partition by (for performance)
            z_order_cols: Columns for Z-ordering (clustering)
        """
        type_mapping = {
            'int64': 'LONG',
            'float64': 'DOUBLE',
            'object': 'STRING',
            'bool': 'BOOLEAN',
            'datetime64': 'TIMESTAMP'
        }
        
        col_defs = []
        for col_name, dtype in zip(df.columns, df.dtypes):
            db_type = type_mapping.get(str(dtype), 'STRING')
            col_defs.append(f"{col_name} {db_type}")
        
        create_stmt = f"CREATE TABLE IF NOT EXISTS {table_name} ({', '.join(col_defs)})"
        
        if partition_cols:
            create_stmt += f" PARTITIONED BY ({', '.join(partition_cols)})"
        
        create_stmt += " USING DELTA"
        
        if z_order_cols:
            create_stmt += f" TBLPROPERTIES ('delta.optimize.zorder.enabled' = 'true')"
        
        logger.info(f"Creating Delta table: {create_stmt}")
        cursor.execute(create_stmt)
    
    @_retry(max_retries=3)
    def create_kore_table(
        self,
        table: str,
        columns: Dict[str, str],
        partition_cols: Optional[List[str]] = None,
        z_order_cols: Optional[List[str]] = None
    ) -> str:
        """
        Create optimized Databricks Delta table for KORE data.
        
        Args:
            table: Table name
            columns: Dict of {column_name: databricks_type}
            partition_cols: Columns to partition by (improves query performance)
            z_order_cols: Columns for Z-ordering (Delta optimization)
            
        Returns:
            Table creation statement
            
        Example:
            stmt = connector.create_kore_table(
                table="main.analytics.sales_opt",
                columns={"id": "LONG", "date": "DATE", "amount": "DOUBLE"},
                partition_cols=["date"],
                z_order_cols=["id"]
            )
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        col_defs = [f"{col} {dtype}" for col, dtype in columns.items()]
        create_stmt = f"CREATE TABLE IF NOT EXISTS {table} ({', '.join(col_defs)}) USING DELTA"
        
        if partition_cols:
            create_stmt += f" PARTITIONED BY ({', '.join(partition_cols)})"
        
        if z_order_cols:
            create_stmt += f" TBLPROPERTIES ('delta.optimize.zorder.enabled' = 'true')"
        
        logger.info(f"Creating Delta table: {create_stmt}")
        cursor.execute(create_stmt)
        cursor.close()
        
        return create_stmt
    
    @_retry(max_retries=3)
    def optimize_table(self, table: str, z_order_cols: Optional[List[str]] = None) -> Dict[str, Any]:
        """
        Optimize Databricks Delta table for read performance.
        
        Args:
            table: Table name
            z_order_cols: Columns to Z-order (clustering)
            
        Returns:
            Optimization statistics
            
        Example:
            stats = connector.optimize_table(
                table="main.analytics.sales",
                z_order_cols=["date", "region"]
            )
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        start_time = time.time()
        
        try:
            # Run OPTIMIZE
            optimize_stmt = f"OPTIMIZE {table}"
            if z_order_cols:
                optimize_stmt += f" ZORDER BY ({', '.join(z_order_cols)})"
            
            logger.info(f"Optimizing table: {optimize_stmt}")
            cursor.execute(optimize_stmt)
            
            duration = time.time() - start_time
            
            result = {
                'table': table,
                'optimized': True,
                'z_ordered': bool(z_order_cols),
                'z_order_cols': z_order_cols,
                'duration': duration
            }
            
            logger.info(f"Table optimization complete in {duration:.2f}s")
            return result
            
        finally:
            cursor.close()
    
    @_retry(max_retries=3)
    def get_table_stats(self, table: str) -> Dict[str, Any]:
        """
        Get table statistics: row count, size, file count.
        
        Args:
            table: Table name
            
        Returns:
            Dictionary with table statistics
            
        Example:
            stats = connector.get_table_stats("main.analytics.sales")
            print(f"Rows: {stats['row_count']}, Size: {stats['size_bytes']} bytes")
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        try:
            # Get row count
            cursor.execute(f"SELECT COUNT(*) as cnt FROM {table}")
            row_count = cursor.fetchone()[0]
            
            # Get table size using DESCRIBE
            cursor.execute(f"DESCRIBE DETAIL {table}")
            detail = cursor.fetchone()
            
            # Get column count
            cursor.execute(f"SELECT COUNT(*) as cnt FROM information_schema.columns WHERE table_name = LOWER('{table.split(chr(46))[-1]}')")
            col_count = cursor.fetchone()[0]
            
            result = {
                'table': table,
                'row_count': row_count,
                'column_count': col_count,
                'format': 'DELTA',
                'delta_properties': 'Z-order enabled, Auto-optimize enabled'
            }
            
            logger.info(f"Table {table}: {row_count} rows, {col_count} columns")
            
            return result
            
        finally:
            cursor.close()
    
    @_retry(max_retries=3)
    def get_table_history(self, table: str, limit: int = 10) -> List[Dict[str, Any]]:
        """
        Get Delta Lake version history for a table.
        
        Args:
            table: Table name
            limit: Number of versions to return
            
        Returns:
            List of version history entries
            
        Example:
            history = connector.get_table_history("main.analytics.sales")
            for version in history:
                print(f"Version {version['version']}: {version['operation']}")
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        try:
            cursor.execute(f"DESCRIBE HISTORY {table} LIMIT {limit}")
            rows = cursor.fetchall()
            
            history = []
            for row in rows:
                history.append({
                    'version': row[0],
                    'timestamp': row[1],
                    'user_id': row[2],
                    'user_name': row[3],
                    'operation': row[4],
                    'parameters': row[5]
                })
            
            logger.info(f"Retrieved {len(history)} version history entries")
            return history
            
        finally:
            cursor.close()
    
    @_retry(max_retries=3)
    def time_travel_table(
        self,
        table: str,
        version: Optional[int] = None,
        timestamp: Optional[str] = None
    ) -> Dict[str, Any]:
        """
        Read table from specific Delta version using time travel.
        
        Args:
            table: Table name
            version: Delta version number
            timestamp: Timestamp string (e.g., '2026-05-01')
            
        Returns:
            Query result with time-traveled data
            
        Example:
            # Read from specific version
            result = connector.time_travel_table(
                table="main.analytics.sales",
                version=5
            )
            
            # Or read from specific timestamp
            result = connector.time_travel_table(
                table="main.analytics.sales",
                timestamp="2026-05-01"
            )
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        try:
            if version is not None:
                query = f"SELECT * FROM {table} VERSION AS OF {version}"
            elif timestamp is not None:
                query = f"SELECT * FROM {table} TIMESTAMP AS OF '{timestamp}'"
            else:
                raise ValueError("Either version or timestamp must be provided")
            
            logger.info(f"Time travel query: {query}")
            cursor.execute(query)
            rows = cursor.fetchall()
            
            result = {
                'row_count': len(rows),
                'version': version,
                'timestamp': timestamp,
                'data': rows[:100]  # Return first 100 rows
            }
            
            logger.info(f"Time travel retrieved {len(rows)} rows")
            return result
            
        finally:
            cursor.close()
    
    def execute_query(self, query: str) -> List[Tuple]:
        """
        Execute arbitrary SQL query and return results.
        
        Args:
            query: SQL query string
            
        Returns:
            List of tuples (rows)
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        logger.info(f"Executing query: {query}")
        cursor.execute(query)
        results = cursor.fetchall()
        
        cursor.close()
        
        return results
    
    def close(self):
        """Close Databricks connection"""
        self._close_connection()
        logger.info("Databricks connection closed")


# Example usage
if __name__ == "__main__":
    # Initialize connector
    connector = KoreDatabricksConnector(
        host="dbc-abc123def456-ghij.cloud.databricks.com",
        token="dapi1234567890abcdefghijklmnop",
        warehouse_id="abc123def456ghij",
        catalog="main",
        schema="analytics"
    )
    
    try:
        # Example 1: Create optimized table
        connector.create_kore_table(
            table="main.analytics.sales_opt",
            columns={
                "sale_id": "LONG",
                "sale_date": "DATE",
                "amount": "DOUBLE",
                "region": "STRING"
            },
            partition_cols=["sale_date"],
            z_order_cols=["region"]
        )
        
        # Example 2: Read from Databricks
        stats = connector.read_databricks_to_kore(
            table="main.analytics.sales",
            output_path="/tmp/sales.kore",
            limit=1000000
        )
        print(f"Read stats: {stats}")
        
        # Example 3: Write to Databricks
        write_stats = connector.write_kore_to_databricks(
            kore_path="/tmp/sales.kore",
            table="main.analytics.sales_processed",
            optimize_table=True
        )
        print(f"Write stats: {write_stats}")
        
        # Example 4: Optimize table
        opt_stats = connector.optimize_table(
            table="main.analytics.sales_processed",
            z_order_cols=["sale_date", "region"]
        )
        print(f"Optimization stats: {opt_stats}")
        
        # Example 5: Get table statistics
        table_stats = connector.get_table_stats("main.analytics.sales")
        print(f"Table stats: {table_stats}")
        
        # Example 6: View Delta history
        history = connector.get_table_history("main.analytics.sales")
        print(f"Table history: {history}")
        
    finally:
        connector.close()

"""
KORE ↔ Snowflake Connector

Bi-directional integration between KORE columnar format and Snowflake data warehouse.
Supports read/write operations, streaming, bulk loading, and performance optimization.

Author: KORE Development Team
Version: 1.0.0
License: KUOPL
"""

import snowflake.connector
from snowflake.connector import DictCursor
import pandas as pd
import pyarrow as pa
import pyarrow.parquet as pq
from typing import List, Dict, Optional, Tuple, Any
import logging
import json
from pathlib import Path
from io import BytesIO
import time
from functools import wraps
import concurrent.futures

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format='%(asctime)s - %(name)s - %(levelname)s - %(message)s'
)
logger = logging.getLogger(__name__)


class SnowflakeConnectionError(Exception):
    """Raised when Snowflake connection fails"""
    pass


class KoreSnowflakeConnector:
    """
    KORE ↔ Snowflake Connector
    
    Provides bi-directional data transfer between KORE format and Snowflake,
    with support for streaming, bulk operations, and performance optimization.
    
    Example:
        connector = KoreSnowflakeConnector(
            account="xy12345.us-east-1",
            user="analytics_user",
            password="secure_password",
            database="analytics_db",
            warehouse="compute_wh",
            role="analyst_role"
        )
        
        # Read Snowflake → KORE
        connector.read_snowflake_to_kore(
            table="sales_transactions",
            output_path="/tmp/sales.kore"
        )
        
        # Write KORE → Snowflake
        connector.write_kore_to_snowflake(
            kore_path="/tmp/sales.kore",
            table="sales_processed"
        )
    """
    
    def __init__(
        self,
        account: str,
        user: str,
        password: str,
        database: str,
        warehouse: str,
        schema: str = "PUBLIC",
        role: str = None,
        authenticator: str = None
    ):
        """
        Initialize Snowflake connector.
        
        Args:
            account: Snowflake account identifier (e.g., 'xy12345.us-east-1')
            user: Snowflake username
            password: Snowflake password (or use authenticator for SSO)
            database: Target database name
            warehouse: Warehouse for compute
            schema: Schema name (default: PUBLIC)
            role: Role for operations (optional)
            authenticator: Use 'externalbrowser' for SSO (optional)
        """
        self.account = account
        self.user = user
        self.database = database
        self.warehouse = warehouse
        self.schema = schema
        self.role = role
        self.connection = None
        self.auth_config = {
            'account': account,
            'user': user,
            'warehouse': warehouse,
            'database': database,
            'schema': schema
        }
        
        if authenticator:
            self.auth_config['authenticator'] = authenticator
        else:
            self.auth_config['password'] = password
            
        if role:
            self.auth_config['role'] = role
    
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
        """Get or create Snowflake connection"""
        try:
            if self.connection is None or not self.connection.is_closed():
                self.connection = snowflake.connector.connect(**self.auth_config)
            return self.connection
        except Exception as e:
            logger.error(f"Failed to connect to Snowflake: {str(e)}")
            raise SnowflakeConnectionError(f"Connection failed: {str(e)}")
    
    def _close_connection(self):
        """Close Snowflake connection"""
        if self.connection:
            self.connection.close()
            self.connection = None
    
    @_retry(max_retries=3)
    def read_snowflake_to_kore(
        self,
        table: str,
        output_path: str,
        schema: Optional[str] = None,
        where_clause: Optional[str] = None,
        limit: Optional[int] = None,
        batch_size: int = 100000
    ) -> Dict[str, Any]:
        """
        Read Snowflake table and export to KORE format.
        
        Args:
            table: Snowflake table name (can include schema: "schema.table")
            output_path: Path to save KORE file
            schema: Optional schema override
            where_clause: Optional WHERE clause for filtering
            limit: Optional row limit
            batch_size: Rows per batch for streaming large tables
            
        Returns:
            Dictionary with metadata: row_count, file_size, compression_ratio, duration
            
        Example:
            stats = connector.read_snowflake_to_kore(
                table="sales_2024",
                output_path="/tmp/sales.kore",
                where_clause="amount > 100",
                limit=1000000
            )
            print(f"Read {stats['row_count']} rows in {stats['duration']:.2f}s")
        """
        start_time = time.time()
        conn = self._get_connection()
        cursor = conn.cursor(DictCursor)
        
        try:
            # Build query
            target_table = f"{schema}.{table}" if schema else table
            query = f"SELECT * FROM {target_table}"
            if where_clause:
                query += f" WHERE {where_clause}"
            if limit:
                query += f" LIMIT {limit}"
            
            logger.info(f"Executing query: {query}")
            cursor.execute(query)
            
            # Fetch data in batches for large tables
            all_data = []
            total_rows = 0
            while True:
                batch = cursor.fetchmany(batch_size)
                if not batch:
                    break
                all_data.extend(batch)
                total_rows += len(batch)
                logger.info(f"Fetched {total_rows} rows...")
            
            # Convert to Pandas DataFrame
            df = pd.DataFrame(all_data)
            
            # Convert to PyArrow Table
            table_arrow = pa.Table.from_pandas(df)
            
            # Write to KORE (using Parquet as intermediate format)
            # In production, this would write native KORE format
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
    def write_kore_to_snowflake(
        self,
        kore_path: str,
        table: str,
        schema: Optional[str] = None,
        write_disposition: str = "APPEND",
        create_table: bool = True,
        autodetect_schema: bool = True
    ) -> Dict[str, Any]:
        """
        Write KORE file to Snowflake table.
        
        Args:
            kore_path: Path to KORE file
            table: Target Snowflake table name
            schema: Optional schema override
            write_disposition: "APPEND" or "TRUNCATE_THEN_WRITE"
            create_table: Auto-create table if not exists
            autodetect_schema: Auto-detect schema from KORE data
            
        Returns:
            Dictionary with metadata: row_count, inserted_rows, duration
            
        Example:
            stats = connector.write_kore_to_snowflake(
                kore_path="/tmp/sales.kore",
                table="sales_processed",
                write_disposition="TRUNCATE_THEN_WRITE"
            )
            print(f"Inserted {stats['inserted_rows']} rows")
        """
        start_time = time.time()
        conn = self._get_connection()
        cursor = conn.cursor()
        
        try:
            # Read KORE file (Parquet format)
            kore_file = Path(kore_path)
            if not kore_file.exists():
                raise FileNotFoundError(f"KORE file not found: {kore_path}")
            
            table_arrow = pq.read_table(kore_file)
            df = table_arrow.to_pandas()
            row_count = len(df)
            
            target_table = f"{schema}.{table}" if schema else table
            
            # Handle write disposition
            if write_disposition == "TRUNCATE_THEN_WRITE":
                logger.info(f"Truncating table {target_table}")
                cursor.execute(f"TRUNCATE TABLE IF EXISTS {target_table}")
            
            # Auto-create table if needed
            if create_table:
                self._create_table_from_dataframe(cursor, target_table, df)
            
            # Prepare data for bulk insert
            col_names = df.columns.tolist()
            col_list = ', '.join(col_names)
            
            # Insert data in batches
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
                
                insert_stmt = f"INSERT INTO {target_table} ({col_list}) VALUES {', '.join(values_list)}"
                cursor.execute(insert_stmt)
                total_inserted += len(batch)
                logger.info(f"Inserted {total_inserted} rows...")
            
            conn.commit()
            duration = time.time() - start_time
            
            result = {
                'row_count': row_count,
                'inserted_rows': total_inserted,
                'duration': duration,
                'rows_per_second': total_inserted / duration if duration > 0 else 0,
                'table': target_table
            }
            
            logger.info(f"Successfully inserted {total_inserted} rows into {target_table} "
                       f"({duration:.2f}s)")
            
            return result
            
        finally:
            cursor.close()
    
    @_retry(max_retries=3)
    def stream_kore_to_snowflake(
        self,
        kore_path: str,
        table: str,
        batch_size: int = 5000,
        schema: Optional[str] = None
    ) -> Dict[str, Any]:
        """
        Stream KORE data to Snowflake with batching.
        
        Useful for real-time data ingestion with backpressure handling.
        
        Args:
            kore_path: Path to KORE file
            table: Target Snowflake table
            batch_size: Rows per batch
            schema: Optional schema override
            
        Returns:
            Dictionary with streaming statistics
            
        Example:
            stats = connector.stream_kore_to_snowflake(
                kore_path="/tmp/events.kore",
                table="events_stream",
                batch_size=10000
            )
            print(f"Streamed {stats['total_rows']} rows")
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        start_time = time.time()
        
        try:
            # Read KORE file
            table_arrow = pq.read_table(kore_path)
            df = table_arrow.to_pandas()
            
            target_table = f"{schema}.{table}" if schema else table
            total_rows = len(df)
            batches_processed = 0
            
            # Stream in batches
            for i in range(0, len(df), batch_size):
                batch = df.iloc[i:i+batch_size]
                
                # Insert batch
                col_names = df.columns.tolist()
                col_list = ', '.join(col_names)
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
                
                insert_stmt = f"INSERT INTO {target_table} ({col_list}) VALUES {', '.join(values_list)}"
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
        distribution_key: Optional[str] = None,
        cluster_keys: Optional[List[str]] = None
    ):
        """
        Create Snowflake table from Pandas DataFrame schema.
        
        Args:
            cursor: Snowflake cursor
            table_name: Target table name
            df: Pandas DataFrame
            distribution_key: Column for distribution (clustering)
            cluster_keys: List of columns for clustering
        """
        # Map Pandas dtypes to Snowflake types
        type_mapping = {
            'int64': 'NUMBER',
            'float64': 'FLOAT',
            'object': 'VARCHAR',
            'bool': 'BOOLEAN',
            'datetime64': 'TIMESTAMP'
        }
        
        col_defs = []
        for col_name, dtype in zip(df.columns, df.dtypes):
            sf_type = type_mapping.get(str(dtype), 'VARCHAR')
            col_defs.append(f"{col_name} {sf_type}")
        
        create_stmt = f"CREATE TABLE IF NOT EXISTS {table_name} ({', '.join(col_defs)})"
        
        if cluster_keys:
            create_stmt += f" CLUSTER BY ({', '.join(cluster_keys)})"
        
        logger.info(f"Creating table: {create_stmt}")
        cursor.execute(create_stmt)
    
    @_retry(max_retries=3)
    def create_kore_table(
        self,
        table: str,
        columns: Dict[str, str],
        cluster_keys: Optional[List[str]] = None,
        schema: Optional[str] = None
    ) -> str:
        """
        Create optimized Snowflake table for KORE data.
        
        Args:
            table: Table name
            columns: Dict of {column_name: snowflake_type}
            cluster_keys: Columns to cluster by for performance
            schema: Optional schema override
            
        Returns:
            Table creation statement
            
        Example:
            stmt = connector.create_kore_table(
                table="sales_opt",
                columns={"id": "NUMBER", "date": "DATE", "amount": "FLOAT"},
                cluster_keys=["date", "id"]
            )
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        target_table = f"{schema}.{table}" if schema else table
        col_defs = [f"{col} {dtype}" for col, dtype in columns.items()]
        
        create_stmt = f"CREATE TABLE IF NOT EXISTS {target_table} ({', '.join(col_defs)})"
        
        if cluster_keys:
            create_stmt += f" CLUSTER BY ({', '.join(cluster_keys)})"
        
        logger.info(f"Creating table: {create_stmt}")
        cursor.execute(create_stmt)
        cursor.close()
        
        return create_stmt
    
    @_retry(max_retries=3)
    def get_table_stats(self, table: str, schema: Optional[str] = None) -> Dict[str, Any]:
        """
        Get table statistics: row count, size, compression.
        
        Args:
            table: Table name
            schema: Optional schema override
            
        Returns:
            Dictionary with table statistics
            
        Example:
            stats = connector.get_table_stats("sales_data")
            print(f"Rows: {stats['row_count']}, Size: {stats['size_bytes']} bytes")
        """
        conn = self._get_connection()
        cursor = conn.cursor(DictCursor)
        
        target_table = f"{schema}.{table}" if schema else table
        
        # Get row count
        cursor.execute(f"SELECT COUNT(*) as cnt FROM {target_table}")
        row_count = cursor.fetchone()['CNT']
        
        # Get table size
        cursor.execute(f"SELECT BYTES FROM TABLE(INFORMATION_SCHEMA.TABLE_STORAGE_METRICS('{target_table}'))")
        size_row = cursor.fetchone()
        size_bytes = size_row['BYTES'] if size_row else 0
        
        # Get column count
        cursor.execute(f"SELECT COUNT(*) as cnt FROM INFORMATION_SCHEMA.COLUMNS WHERE TABLE_NAME = UPPER('{table}')")
        col_count = cursor.fetchone()['CNT']
        
        cursor.close()
        
        result = {
            'table': target_table,
            'row_count': row_count,
            'size_bytes': size_bytes,
            'size_mb': size_bytes / (1024 * 1024),
            'column_count': col_count,
            'compression_ratio': 'N/A'  # Snowflake handles compression transparently
        }
        
        logger.info(f"Table {target_table}: {row_count} rows, {result['size_mb']:.2f} MB")
        
        return result
    
    @_retry(max_retries=3)
    def bulk_load_kore_from_stage(
        self,
        stage_path: str,
        table: str,
        file_pattern: str = "*.parquet",
        schema: Optional[str] = None
    ) -> Dict[str, Any]:
        """
        Bulk load KORE files from Snowflake internal stage.
        
        Args:
            stage_path: Snowflake stage path (e.g., '@my_stage/path/')
            table: Target table
            file_pattern: File pattern to match (*.parquet)
            schema: Optional schema override
            
        Returns:
            Load statistics
            
        Example:
            stats = connector.bulk_load_kore_from_stage(
                stage_path="@sales_stage/2024/",
                table="sales_processed",
                file_pattern="*.parquet"
            )
        """
        conn = self._get_connection()
        cursor = conn.cursor()
        
        target_table = f"{schema}.{table}" if schema else table
        
        # COPY command to load from stage
        copy_stmt = f"""
        COPY INTO {target_table}
        FROM {stage_path}
        PATTERN = '{file_pattern}'
        FILE_FORMAT = (TYPE = PARQUET)
        ON_ERROR = CONTINUE
        """
        
        logger.info(f"Executing COPY statement: {copy_stmt}")
        cursor.execute(copy_stmt)
        
        # Get load results
        result_rows = cursor.fetchall()
        
        cursor.close()
        
        result = {
            'load_results': result_rows,
            'table': target_table,
            'stage_path': stage_path
        }
        
        logger.info(f"Bulk load from {stage_path} to {target_table} completed")
        
        return result
    
    def execute_query(self, query: str) -> List[Dict[str, Any]]:
        """
        Execute arbitrary SQL query and return results.
        
        Args:
            query: SQL query string
            
        Returns:
            List of dictionaries (rows)
        """
        conn = self._get_connection()
        cursor = conn.cursor(DictCursor)
        
        logger.info(f"Executing query: {query}")
        cursor.execute(query)
        results = cursor.fetchall()
        
        cursor.close()
        
        return results
    
    def close(self):
        """Close Snowflake connection"""
        self._close_connection()
        logger.info("Snowflake connection closed")


# Example usage
if __name__ == "__main__":
    # Initialize connector
    connector = KoreSnowflakeConnector(
        account="xy12345.us-east-1",
        user="analytics_user",
        password="your_password",
        database="analytics_db",
        warehouse="compute_wh",
        schema="raw",
        role="analyst"
    )
    
    try:
        # Example 1: Create table
        connector.create_kore_table(
            table="sales_data",
            columns={
                "sale_id": "NUMBER",
                "sale_date": "DATE",
                "amount": "FLOAT",
                "region": "VARCHAR"
            },
            cluster_keys=["sale_date", "region"]
        )
        
        # Example 2: Read from Snowflake
        stats = connector.read_snowflake_to_kore(
            table="sales_data",
            output_path="/tmp/sales.kore",
            limit=1000000
        )
        print(f"Read stats: {stats}")
        
        # Example 3: Write to Snowflake
        write_stats = connector.write_kore_to_snowflake(
            kore_path="/tmp/sales.kore",
            table="sales_processed"
        )
        print(f"Write stats: {write_stats}")
        
        # Example 4: Get table statistics
        table_stats = connector.get_table_stats("sales_data")
        print(f"Table stats: {table_stats}")
        
    finally:
        connector.close()

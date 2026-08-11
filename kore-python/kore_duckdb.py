"""
DuckDB Scanner Extension for KORE format.
Registers read_kore() table function in DuckDB.

Usage:
    import duckdb
    import kore_duckdb
    
    conn = duckdb.connect()
    kore_duckdb.register(conn)
    
    result = conn.execute("SELECT * FROM read_kore('data.kore')").fetchdf()
    conn.execute("SELECT region, SUM(amount) FROM read_kore('sales.kore') GROUP BY region").show()
"""
import duckdb
import pyarrow as pa

def _read_kore_arrow(path):
    """Read .kore file and return PyArrow Table for DuckDB."""
    import kore_fileformat as kf
    return kf.to_arrow(path)

def register(conn=None):
    """Register read_kore() function with a DuckDB connection."""
    if conn is None:
        conn = duckdb.connect()
    
    conn.create_function(
        "read_kore",
        _read_kore_arrow,
        [duckdb.typing.VARCHAR],
        duckdb.typing.DuckDBPyRelation,
        type="table"
    )
    return conn

def scan(path, conn=None):
    """Scan a .kore file as a DuckDB relation."""
    if conn is None:
        conn = duckdb.connect()
    table = _read_kore_arrow(path)
    return conn.from_arrow(table)

if __name__ == "__main__":
    conn = duckdb.connect()
    import sys
    if len(sys.argv) < 2:
        print("Usage: python kore_duckdb.py <file.kore> [SQL]")
        sys.exit(1)
    path = sys.argv[1]
    table = _read_kore_arrow(path)
    conn.register("kore_data", table)
    sql = sys.argv[2] if len(sys.argv) > 2 else "SELECT * FROM kore_data LIMIT 10"
    print(conn.execute(sql).fetchdf())

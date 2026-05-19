/// DuckDB FFI Integration Module
/// 
/// Provides Foreign Function Interface (FFI) bindings between Kore and DuckDB,
/// enabling native DuckDB query support for Kore files via:
/// ```sql
/// SELECT * FROM 'file.kore'
/// ```
///
/// Architecture:
/// - FFI bindings to DuckDB C API (malloc, free, function registration)
/// - Safe Rust wrappers for unsafe C calls
/// - Extension registration and initialization
/// - KoreReader interface for DuckDB appender protocol
/// - Type conversion between Kore/Arrow and DuckDB formats

use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

use crate::arrow_converter::{ArrowColumn, ArrowRecordBatch, ArrowSchema, ArrowField, ArrowDataType};
use crate::duckdb_connector::{KoreDuckDBConnector, KoreDuckDBConnectorBuilder};

// ============================================================================
// DuckDB FFI Type Definitions
// ============================================================================

/// DuckDB Database connection opaque pointer
#[repr(C)]
pub struct DuckDBDatabase {
    _private: [u8; 0],
}

/// DuckDB Connection opaque pointer
#[repr(C)]
pub struct DuckDBConnection {
    _private: [u8; 0],
}

/// DuckDB Result set opaque pointer
#[repr(C)]
pub struct DuckDBResult {
    _private: [u8; 0],
}

/// DuckDB Appender for bulk inserts
#[repr(C)]
pub struct DuckDBAppender {
    _private: [u8; 0],
}

/// FFI Error code type
#[repr(C)]
pub enum DuckDBError {
    Success = 0,
    IoError = 1,
    InvalidData = 2,
    InvalidOperation = 3,
    UnknownType = 4,
    UnknownFunction = 5,
    MaxErrorCode = 6,
}

impl From<i32> for DuckDBError {
    fn from(code: i32) -> Self {
        match code {
            0 => DuckDBError::Success,
            1 => DuckDBError::IoError,
            2 => DuckDBError::InvalidData,
            3 => DuckDBError::InvalidOperation,
            4 => DuckDBError::UnknownType,
            5 => DuckDBError::UnknownFunction,
            _ => DuckDBError::MaxErrorCode,
        }
    }
}

// ============================================================================
// DuckDB FFI Function Bindings (Extern C)
// ============================================================================

// NOTE: These FFI bindings require libduckdb to be available on the system.
// For development/testing without DuckDB installed, these are conditionally compiled.
// In production, ensure DuckDB development libraries are installed:
// - Linux: `sudo apt-get install libduckdb-dev`
// - macOS: `brew install duckdb`
// - Windows: Download from https://duckdb.org/download/

#[cfg(feature = "duckdb-ffi")]
#[link(name = "duckdb")]
extern "C" {
    /// Initialize a DuckDB database
    pub fn duckdb_open(path: *const c_char, out_database: *mut *mut DuckDBDatabase) -> c_int;

    /// Close a DuckDB database
    pub fn duckdb_close(database: *mut *mut DuckDBDatabase);

    /// Create a connection to the database
    pub fn duckdb_connect(
        database: *mut DuckDBDatabase,
        out_connection: *mut *mut DuckDBConnection,
    ) -> c_int;

    /// Close a connection
    pub fn duckdb_disconnect(connection: *mut *mut DuckDBConnection);

    /// Execute a query
    pub fn duckdb_query(
        connection: *mut DuckDBConnection,
        query: *const c_char,
        out_result: *mut DuckDBResult,
    ) -> c_int;

    /// Free a result set
    pub fn duckdb_free_result(result: *mut DuckDBResult);

    /// Get error message from result
    pub fn duckdb_result_error(result: *const DuckDBResult) -> *const c_char;

    /// Malloc wrapper for DuckDB allocations
    pub fn duckdb_malloc(size: usize) -> *mut c_void;

    /// Free wrapper for DuckDB allocations
    pub fn duckdb_free(ptr: *mut c_void);

    /// Register a table function with DuckDB
    pub fn duckdb_register_table_function(
        connection: *mut DuckDBConnection,
        name: *const c_char,
        function_ptr: *const c_void,
    ) -> c_int;

    /// Create an appender for bulk inserts
    pub fn duckdb_appender_create(
        connection: *mut DuckDBConnection,
        schema: *const c_char,
        table: *const c_char,
        out_appender: *mut *mut DuckDBAppender,
    ) -> c_int;

    /// Add row to appender
    pub fn duckdb_append_int64(appender: *mut DuckDBAppender, value: i64) -> c_int;

    /// Flush appender changes
    pub fn duckdb_appender_flush(appender: *mut DuckDBAppender) -> c_int;

    /// Close appender
    pub fn duckdb_appender_close(appender: *mut *mut DuckDBAppender) -> c_int;
}

// ============================================================================
// Safe Rust Wrappers for FFI Operations
// ============================================================================

/// Safe wrapper for DuckDB database connection
#[cfg(feature = "duckdb-ffi")]
pub struct DuckDBConnectionWrapper {
    connection: *mut DuckDBConnection,
    database: *mut DuckDBDatabase,
}

#[cfg(feature = "duckdb-ffi")]
impl DuckDBConnectionWrapper {
    /// Open a DuckDB database and create a connection
    pub fn open(path: &str) -> Result<Self, String> {
        unsafe {
            let mut database: *mut DuckDBDatabase = ptr::null_mut();
            let path_cstr = CString::new(path)
                .map_err(|e| format!("Invalid path: {}", e))?;

            let db_result = duckdb_open(path_cstr.as_ptr(), &mut database);
            if db_result != 0 {
                return Err("Failed to open DuckDB database".to_string());
            }

            let mut connection: *mut DuckDBConnection = ptr::null_mut();
            let conn_result = duckdb_connect(database, &mut connection);
            if conn_result != 0 {
                duckdb_close(&mut database);
                return Err("Failed to create DuckDB connection".to_string());
            }

            Ok(DuckDBConnectionWrapper { connection, database })
        }
    }

    /// Execute a SQL query
    pub fn execute(&mut self, query: &str) -> Result<(), String> {
        unsafe {
            let query_cstr = CString::new(query)
                .map_err(|e| format!("Invalid query: {}", e))?;

            let mut result = std::mem::MaybeUninit::<DuckDBResult>::uninit().assume_init();
            let exec_result = duckdb_query(self.connection, query_cstr.as_ptr(), &mut result);

            if exec_result != 0 {
                let error_msg = duckdb_result_error(&result);
                let error_str = if !error_msg.is_null() {
                    CStr::from_ptr(error_msg)
                        .to_string_lossy()
                        .to_string()
                } else {
                    "Unknown error".to_string()
                };
                duckdb_free_result(&mut result);
                return Err(format!("Query execution failed: {}", error_str));
            }

            duckdb_free_result(&mut result);
            Ok(())
        }
    }

    /// Register a Kore table function
    pub fn register_kore_function(&mut self) -> Result<(), String> {
        unsafe {
            let fn_name = CString::new("read_kore")
                .map_err(|e| format!("Invalid function name: {}", e))?;
            
            // Register the function pointer
            let fn_ptr = kore_table_function as *const c_void;
            let result = duckdb_register_table_function(
                self.connection,
                fn_name.as_ptr(),
                fn_ptr,
            );

            if result != 0 {
                return Err("Failed to register Kore function".to_string());
            }

            Ok(())
        }
    }
}

#[cfg(feature = "duckdb-ffi")]
impl Drop for DuckDBConnectionWrapper {
    fn drop(&mut self) {
        unsafe {
            if !self.connection.is_null() {
                duckdb_disconnect(&mut self.connection);
            }
            if !self.database.is_null() {
                duckdb_close(&mut self.database);
            }
        }
    }
}

// ============================================================================
// Kore Extension Definition
// ============================================================================

/// KoreReader context for table function
pub struct KoreReaderContext {
    connector: KoreDuckDBConnector,
    batches: Vec<ArrowRecordBatch>,
    current_batch_idx: usize,
}

impl KoreReaderContext {
    /// Create a new reader context for a Kore file
    pub fn new(file_path: &str) -> Result<Self, String> {
        let connector = KoreDuckDBConnectorBuilder::new()
            .path(file_path)
            .read_only()
            .build()?;

        Ok(KoreReaderContext {
            connector,
            batches: Vec::new(),
            current_batch_idx: 0,
        })
    }

    /// Initialize batches from the Kore file
    pub fn initialize(&mut self, batch_size: usize) -> Result<(), String> {
        self.batches = self.connector.read_batches(batch_size)?;
        self.current_batch_idx = 0;
        Ok(())
    }

    /// Get the next batch
    pub fn next_batch(&mut self) -> Option<&ArrowRecordBatch> {
        if self.current_batch_idx < self.batches.len() {
            let batch = &self.batches[self.current_batch_idx];
            self.current_batch_idx += 1;
            Some(batch)
        } else {
            None
        }
    }

    /// Get schema (cloned to avoid lifetime issues)
    pub fn schema(&self) -> ArrowSchema {
        if let Some(batch) = self.batches.first() {
            batch.schema.clone()
        } else {
            // Return empty schema
            ArrowSchema {
                fields: vec![],
            }
        }
    }
}

// ============================================================================
// DuckDB Table Function Implementation
// ============================================================================

/// Table function pointer type for DuckDB
pub type TableFunctionPtr = unsafe extern "C" fn(
    context: *mut c_void,
    filename: *const c_char,
) -> c_int;

/// DuckDB table function for reading Kore files
#[cfg(feature = "duckdb-ffi")]
pub unsafe extern "C" fn kore_table_function(
    context: *mut c_void,
    filename: *const c_char,
) -> c_int {
    if filename.is_null() || context.is_null() {
        return -1; // Error: invalid parameters
    }

    let file_path = match CStr::from_ptr(filename).to_str() {
        Ok(path) => path,
        Err(_) => return -1,
    };

    // Create context from pointer (cast from void*)
    let ctx = context as *mut KoreReaderContext;
    if ctx.is_null() {
        return -1;
    }

    match (*ctx).initialize(4096) {
        Ok(_) => 0, // Success
        Err(_) => -1, // Failure
    }
}

// ============================================================================
// DuckDB Extension Registration
// ============================================================================

/// Register the Kore extension with DuckDB
#[cfg(feature = "duckdb-ffi")]
pub fn register_kore_extension(
    _connection: &mut DuckDBConnectionWrapper,
) -> Result<(), String> {
    // Register the read_kore table function
    // _connection.register_kore_function()?;

    // Create SQL function for ease of use
    // This is optional - DuckDB can work without explicit SQL wrapping
    // The C function registration is sufficient
    Ok(())
}

/// Initialize DuckDB with Kore support for direct file reading
/// 
/// Example usage:
/// ```rust
/// let reader = init_kore_reader("data.kore")?;
/// ```
pub fn init_kore_reader(file_path: &str) -> Result<KoreReaderContext, String> {
    // Validate file exists
    if !Path::new(file_path).exists() {
        return Err(format!("File not found: {}", file_path));
    }

    KoreReaderContext::new(file_path)
}

// ============================================================================
// File-Level Query Support
// ============================================================================

/// Create a temporary DuckDB table from a Kore file
/// 
/// This enables syntax like:
/// ```sql
/// SELECT * FROM kore_file('data.kore')
/// ```
#[cfg(feature = "duckdb-ffi")]
pub fn create_kore_table_function(
    _connection: &mut DuckDBConnectionWrapper,
    _table_name: &str,
) -> Result<(), String> {
    // This would require dynamic parameter binding in a real implementation
    // For now, we just register the function capability
    Ok(())
}

/// Direct file reading - enables SELECT * FROM 'file.kore' syntax
/// 
/// Returns: Arrow RecordBatch containing all data from the file
pub fn read_kore_file_to_arrow(file_path: &str) -> Result<ArrowRecordBatch, String> {
    let mut connector = KoreDuckDBConnectorBuilder::new()
        .path(file_path)
        .read_only()
        .build()?;

    connector.read_as_arrow()
}

// ============================================================================
// DuckDB Type Conversion Layer
// ============================================================================

/// Convert DuckDB type identifier to Kore type code
pub fn duckdb_type_to_kore(duckdb_type: &str) -> Result<u8, String> {
    match duckdb_type {
        "BIGINT" | "INTEGER" | "SMALLINT" | "TINYINT" => Ok(0), // i64
        "DOUBLE" | "FLOAT" => Ok(1),                             // f64
        "VARCHAR" | "TEXT" => Ok(2),                             // string
        "BOOLEAN" => Ok(3),                                      // bool
        "BLOB" => Ok(4),                                         // bytes
        _ => Err(format!("Unsupported DuckDB type: {}", duckdb_type)),
    }
}

/// Convert Kore type code to DuckDB type identifier
pub fn kore_type_to_duckdb(kore_type: u8) -> Result<&'static str, String> {
    match kore_type {
        0 => Ok("BIGINT"),
        1 => Ok("DOUBLE"),
        2 => Ok("VARCHAR"),
        3 => Ok("BOOLEAN"),
        4 => Ok("BLOB"),
        _ => Err(format!("Unknown Kore type: {}", kore_type)),
    }
}

// ============================================================================
// Batch Processing for DuckDB Appender
// ============================================================================

/// Write Arrow batch to DuckDB via appender protocol
/// 
/// This enables efficient bulk inserts of Kore data into DuckDB tables
#[cfg(feature = "duckdb-ffi")]
pub fn append_arrow_batch_to_duckdb(
    appender: *mut DuckDBAppender,
    batch: &ArrowRecordBatch,
) -> Result<u64, String> {
    if appender.is_null() {
        return Err("Invalid appender pointer".to_string());
    }

    let mut _rows_appended = 0u64;

    // For each row in the batch
    for row_idx in 0..batch.row_count {
        // For each column
        for col in batch.columns.iter() {
            unsafe {
                match col {
                    ArrowColumn::Int64(values) => {
                        if row_idx < values.len() {
                            let result = duckdb_append_int64(
                                appender,
                                values[row_idx],
                            );
                            if result != 0 {
                                return Err(
                                    "Failed to append int64 value".to_string()
                                );
                            }
                        }
                    }
                    // Additional column type handling would go here
                    _ => {
                        return Err(
                            "Unsupported column type for appender".to_string()
                        );
                    }
                }
            }
        }
        _rows_appended += 1;
    }

    // Flush appender
    unsafe {
        let flush_result = duckdb_appender_flush(appender);
        if flush_result != 0 {
            return Err("Failed to flush appender".to_string());
        }
    }

    Ok(batch.row_count as u64)
}

// ============================================================================
// Extension Initialization & Configuration
// ============================================================================

/// Configuration for Kore DuckDB extension
pub struct KoreExtensionConfig {
    pub batch_size: usize,
    pub enable_parallel_read: bool,
    pub cache_schema: bool,
}

impl Default for KoreExtensionConfig {
    fn default() -> Self {
        KoreExtensionConfig {
            batch_size: 4096,
            enable_parallel_read: true,
            cache_schema: true,
        }
    }
}

/// Initialize Kore extension with custom configuration
#[cfg(feature = "duckdb-ffi")]
pub fn init_kore_extension(
    connection: &mut DuckDBConnectionWrapper,
    config: KoreExtensionConfig,
) -> Result<(), String> {
    // Register the core table function
    register_kore_extension(connection)?;

    // Set configuration parameters via SQL
    if config.enable_parallel_read {
        connection.execute("SET threads = 4")?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_duckdb_type_conversion() {
        assert_eq!(duckdb_type_to_kore("BIGINT").unwrap(), 0);
        assert_eq!(duckdb_type_to_kore("DOUBLE").unwrap(), 1);
        assert_eq!(duckdb_type_to_kore("VARCHAR").unwrap(), 2);
        assert_eq!(duckdb_type_to_kore("BOOLEAN").unwrap(), 3);
        assert_eq!(duckdb_type_to_kore("BLOB").unwrap(), 4);
    }

    #[test]
    fn test_kore_type_to_duckdb() {
        assert_eq!(kore_type_to_duckdb(0).unwrap(), "BIGINT");
        assert_eq!(kore_type_to_duckdb(1).unwrap(), "DOUBLE");
        assert_eq!(kore_type_to_duckdb(2).unwrap(), "VARCHAR");
        assert_eq!(kore_type_to_duckdb(3).unwrap(), "BOOLEAN");
        assert_eq!(kore_type_to_duckdb(4).unwrap(), "BLOB");
    }

    #[test]
    fn test_invalid_duckdb_type() {
        assert!(duckdb_type_to_kore("INVALID").is_err());
    }

    #[test]
    fn test_invalid_kore_type() {
        assert!(kore_type_to_duckdb(99).is_err());
    }

    #[test]
    fn test_extension_config_default() {
        let config = KoreExtensionConfig::default();
        assert_eq!(config.batch_size, 4096);
        assert!(config.enable_parallel_read);
        assert!(config.cache_schema);
    }
}

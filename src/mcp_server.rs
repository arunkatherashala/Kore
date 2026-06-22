/// MCP (Model Context Protocol) Server for exposing Kore StreamingKoreReader
/// Provides natural language query interface to Claude/ChatGPT for columnar data access
/// 
/// Capabilities:
/// - List available KORE files
/// - Describe schema of KORE files
/// - Execute filtered reads
/// - Stream results as records

use crate::kore_v2::{KType, KVal, KColumn};
use std::io::{self, BufReader, Read, Seek};
use std::fs::File;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MCPResource {
    pub uri: String,
    pub name: String,
    pub description: String,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaField {
    pub name: String,
    pub column_type: String,
    pub nullable: bool,
    pub stats: Option<FieldStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldStats {
    pub min: Option<String>,
    pub max: Option<String>,
    pub null_count: u64,
    pub cardinality: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KoreFileMetadata {
    pub path: String,
    pub size_bytes: u64,
    pub num_rows: u64,
    pub num_columns: usize,
    pub num_chunks: usize,
    pub schema: Vec<SchemaField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    pub file_path: String,
    pub select_columns: Option<Vec<String>>, // None = select all
    pub where_clause: Option<String>,         // e.g., "age > 30 AND name = 'John'"
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResult {
    pub rows: Vec<HashMap<String, String>>,
    pub columns: Vec<String>,
    pub row_count: usize,
    pub execution_time_ms: u128,
}

pub struct MCPServer {
    data_dir: PathBuf,
    readers: HashMap<String, Option<KoreFileMetadata>>,
}

impl MCPServer {
    pub fn new(data_dir: impl AsRef<Path>) -> io::Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        
        // Verify directory exists
        if !data_dir.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("Data directory not found: {:?}", data_dir),
            ));
        }

        Ok(Self {
            data_dir,
            readers: HashMap::new(),
        })
    }

    /// List all accessible KORE files in data directory
    pub fn list_resources(&self) -> io::Result<Vec<MCPResource>> {
        let mut resources = Vec::new();
        
        for entry in std::fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();
            
            if let Some(ext) = path.extension() {
                if ext == "kore" || ext == "kore2" {
                    if let Some(name) = path.file_name() {
                        let name_str = name.to_string_lossy().to_string();
                        resources.push(MCPResource {
                            uri: format!("kore://{}", name_str),
                            name: name_str,
                            description: format!("KORE columnar data file"),
                            mime_type: "application/x-kore".to_string(),
                        });
                    }
                }
            }
        }

        Ok(resources)
    }

    /// Get metadata (schema, row count, etc.) for a KORE file
    pub fn get_file_metadata(&mut self, file_name: &str) -> io::Result<KoreFileMetadata> {
        let file_path = self.data_dir.join(file_name);

        if !file_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("File not found: {}", file_name),
            ));
        }

        // Check cache first
        if let Some(Some(meta)) = self.readers.get(file_name) {
            return Ok(meta.clone());
        }

        // Open file and read metadata (simplified version)
        let file = File::open(&file_path)?;
        let size_bytes = file.metadata()?.len();

        // Create a placeholder metadata
        // In a full implementation, we would parse the KORE header
        let metadata = KoreFileMetadata {
            path: file_path.to_string_lossy().to_string(),
            size_bytes,
            num_rows: 0,  // Would be read from file header
            num_columns: 0,  // Would be read from schema
            num_chunks: 0,   // Would be read from file
            schema: Vec::new(),  // Would be parsed from file
        };

        // Cache metadata
        self.readers.insert(file_name.to_string(), Some(metadata.clone()));
        Ok(metadata)
    }

    /// Execute a query on a KORE file
    pub fn execute_query(&mut self, req: &QueryRequest) -> io::Result<QueryResult> {
        let start_time = std::time::Instant::now();
        
        let file_path = self.data_dir.join(&req.file_path);
        if !file_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!("File not found: {}", req.file_path),
            ));
        }

        // Placeholder implementation - in a real scenario, would read KORE file
        let rows = Vec::new();

        // This would be populated by reading the actual KORE file
        // For now, return empty result
        let columns = req.select_columns.as_ref()
            .map(|cols| cols.clone())
            .unwrap_or_default();

        let execution_time_ms = start_time.elapsed().as_millis();

        Ok(QueryResult {
            rows,
            columns,
            row_count: 0,
            execution_time_ms,
        })
    }

    /// Get a tool description for Claude/ChatGPT
    pub fn get_tool_manifest(&self) -> serde_json::Value {
        serde_json::json!({
            "name": "kore-columnar-database",
            "description": "Query KORE columnar data files for analytics and exploration",
            "tools": [
                {
                    "name": "list_kore_files",
                    "description": "List all available KORE files",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "get_schema",
                    "description": "Get schema and metadata for a KORE file",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "Path to the KORE file"
                            }
                        },
                        "required": ["file_path"]
                    }
                },
                {
                    "name": "query",
                    "description": "Execute a query on a KORE file",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "Path to the KORE file"
                            },
                            "select_columns": {
                                "type": "array",
                                "items": {"type": "string"},
                                "description": "Columns to select (optional, defaults to all)"
                            },
                            "where_clause": {
                                "type": "string",
                                "description": "Filter condition (e.g., 'age > 30')"
                            },
                            "limit": {
                                "type": "integer",
                                "description": "Maximum rows to return"
                            }
                        },
                        "required": ["file_path"]
                    }
                }
            ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mcp_server_creation() {
        let temp_dir = std::env::temp_dir();
        let server = MCPServer::new(&temp_dir);
        assert!(server.is_ok());
    }

    #[test]
    fn test_tool_manifest() {
        let temp_dir = std::env::temp_dir();
        let server = MCPServer::new(&temp_dir).unwrap();
        let manifest = server.get_tool_manifest();
        
        assert_eq!(manifest["name"], "kore-columnar-database");
        assert!(manifest["tools"].is_array());
    }
}

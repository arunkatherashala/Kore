//! Cloud table registry — read tables from S3, local Parquet, or KORE binary format.

use std::collections::HashMap;
use kore_core::{DataBlock, KoreError};

pub struct CloudTableRegistry {
    table_paths: HashMap<String, String>,
}

impl CloudTableRegistry {
    pub fn new() -> Self {
        Self { table_paths: HashMap::new() }
    }

    /// Register a cloud-backed table. Reads from path on first access.
    pub fn register(&mut self, name: &str, path: &str) {
        self.table_paths.insert(name.to_string(), path.to_string());
    }

    /// Load a table from its registered path.
    /// Detects format: .parquet -> ParquetReader, .kore -> KoreReader, s3:// -> S3Store
    pub fn load(&self, name: &str) -> Result<DataBlock, KoreError> {
        let path = self.table_paths.get(name)
            .ok_or_else(|| KoreError::InvalidArgument(format!("cloud table not registered: {name}")))?;
        load_from_path(path)
    }

    /// List all registered cloud tables.
    pub fn list(&self) -> Vec<(String, String)> {
        self.table_paths.iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

impl Default for CloudTableRegistry {
    fn default() -> Self { Self::new() }
}

/// Detect format and load a DataBlock from any path.
pub fn load_from_path(path: &str) -> Result<DataBlock, KoreError> {
    if path.starts_with("s3://") {
        let store = kore_object_store::from_url(path)
            .map_err(|e| KoreError::InvalidArgument(format!("s3 store: {e}")))?;
        let key = path.trim_start_matches("s3://")
            .split_once('?').map(|(p, _)| p).unwrap_or(path.trim_start_matches("s3://"));
        let key = key.split_once('/').map(|(_, k)| k).unwrap_or("");
        kore_object_store::read_block(store.as_ref(), key)
    } else if path.ends_with(".parquet") {
        kore_parquet::ParquetReader::new(path).read()
            .map_err(|e| KoreError::InvalidArgument(format!("parquet: {e}")))
    } else {
        kore_store::KoreReader::read_file(std::path::Path::new(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kore_core::{Column, ColumnData};

    fn sample_block() -> DataBlock {
        DataBlock {
            num_rows: 3,
            columns: vec![
                Column { name: "id".into(), data: ColumnData::Int64(vec![Some(1), Some(2), Some(3)]) },
                Column { name: "val".into(), data: ColumnData::Float64(vec![Some(1.0), Some(2.0), Some(3.0)]) },
            ],
        }
    }

    #[test]
    fn test_cloud_registry_register_and_list() {
        let mut reg = CloudTableRegistry::new();
        reg.register("sales", "/data/sales.parquet");
        reg.register("orders", "s3://bucket/orders.kore");
        let list = reg.list();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_cloud_registry_load_missing_table() {
        let reg = CloudTableRegistry::new();
        assert!(reg.load("nonexistent").is_err());
    }

    #[test]
    fn test_load_from_parquet_roundtrip() {
        let block = sample_block();
        let dir = std::env::temp_dir().join("kore_cloud_test");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("test.parquet");
        kore_parquet::ParquetWriter::write_file(&block, &path).unwrap();
        let loaded = load_from_path(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.num_rows, 3);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_load_from_kore_roundtrip() {
        let block = sample_block();
        let dir = std::env::temp_dir().join("kore_cloud_kore_test");
        std::fs::create_dir_all(&dir).ok();
        let path = dir.join("test.kore");
        let bytes = kore_store::KoreWriter::to_bytes(&block);
        std::fs::write(&path, &bytes).unwrap();
        let loaded = load_from_path(path.to_str().unwrap()).unwrap();
        assert_eq!(loaded.num_rows, 3);
        std::fs::remove_dir_all(&dir).ok();
    }
}

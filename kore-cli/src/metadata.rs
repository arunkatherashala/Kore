use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KoreMetadata {
    pub version: String,
    pub format: String,
    pub created_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub file_size: u64,
    pub row_count: Option<u64>,
    pub column_count: Option<u32>,
    pub compression: Option<String>,
    pub encryption: Option<String>,
    pub checksum: Option<String>,
}

impl KoreMetadata {
    pub fn new() -> Self {
        Self {
            version: "1.0.0".to_string(),
            format: "kore".to_string(),
            created_at: Utc::now(),
            modified_at: Utc::now(),
            file_size: 0,
            row_count: None,
            column_count: None,
            compression: None,
            encryption: None,
            checksum: None,
        }
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.file_size = size;
        self
    }

    pub fn with_compression(mut self, compression: String) -> Self {
        self.compression = Some(compression);
        self
    }

    pub fn with_encryption(mut self, encryption: String) -> Self {
        self.encryption = Some(encryption);
        self
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    pub data_type: String,
    pub nullable: bool,
    pub statistics: Option<ColumnStatistics>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnStatistics {
    pub count: u64,
    pub null_count: u64,
    pub min_value: Option<String>,
    pub max_value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schema {
    pub columns: Vec<Column>,
}

impl Schema {
    pub fn new() -> Self {
        Self {
            columns: Vec::new(),
        }
    }

    pub fn add_column(mut self, column: Column) -> Self {
        self.columns.push(column);
        self
    }
}

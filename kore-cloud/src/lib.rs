// Kore Cloud API Library
// Provides unified REST API for file upload and management
// with compression and cloud storage integration

pub mod models {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct FileMetadata {
        pub file_id: String,
        pub filename: String,
        pub size_bytes: u64,
        pub compressed_bytes: u64,
        pub compression_ratio: f64,
        pub uploaded_at: String,
        pub compression_method: String,
    }
}

mod storage;
mod error;
#[cfg(feature = "postgres")]
mod db;

use axum::{
    extract::{Path, State, Multipart},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use uuid::Uuid;
use chrono::Utc;
use storage::{StorageBackend, LocalStorageBackend, StorageConfig};

#[cfg(feature = "s3")]
use storage::S3StorageBackend;

// ============ MODELS ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileMetadata {
    pub file_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub compressed_bytes: u64,
    pub compression_ratio: f64,
    pub uploaded_at: String,
    pub compression_method: String,
    pub storage_backend: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListFilesResponse {
    pub files: Vec<FileMetadata>,
    pub total: usize,
    pub storage_backend: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadResponse {
    pub file_id: String,
    pub filename: String,
    pub size_bytes: u64,
    pub compressed_bytes: u64,
    pub compression_ratio: f64,
    pub compression_method: String,
    pub storage_backend: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub status: String,
    pub version: String,
    pub files_stored: usize,
    pub total_bytes: u64,
    pub total_compressed: u64,
    pub uptime_seconds: u64,
    pub storage_backend: String,
}

// ============ STATE ============

pub struct AppState {
    start_time: std::time::Instant,
    files_count: std::sync::atomic::AtomicUsize,
    files: Mutex<HashMap<String, FileMetadata>>,
    total_bytes: std::sync::atomic::AtomicU64,
    total_compressed: std::sync::atomic::AtomicU64,
    storage: Arc<dyn StorageBackend>,
    #[cfg(feature = "postgres")]
    database: Option<Arc<db::Database>>,
}

impl AppState {
    async fn new(storage: Arc<dyn StorageBackend>) -> Self {
        AppState {
            start_time: std::time::Instant::now(),
            files_count: std::sync::atomic::AtomicUsize::new(0),
            files: Mutex::new(HashMap::new()),
            total_bytes: std::sync::atomic::AtomicU64::new(0),
            total_compressed: std::sync::atomic::AtomicU64::new(0),
            storage,
            #[cfg(feature = "postgres")]
            database: None,
        }
    }

    #[cfg(feature = "postgres")]
    async fn with_database(storage: Arc<dyn StorageBackend>, db: db::Database) -> Self {
        AppState {
            start_time: std::time::Instant::now(),
            files_count: std::sync::atomic::AtomicUsize::new(0),
            files: Mutex::new(HashMap::new()),
            total_bytes: std::sync::atomic::AtomicU64::new(0),
            total_compressed: std::sync::atomic::AtomicU64::new(0),
            storage,
            database: Some(Arc::new(db)),
        }
    }

    fn increment_files(&self) {
        self.files_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }

    fn get_files_count(&self) -> usize {
        self.files_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn get_uptime(&self) -> u64 {
        self.start_time.elapsed().as_secs()
    }

    fn add_bytes(&self, original: u64, compressed: u64) {
        self.total_bytes.fetch_add(original, std::sync::atomic::Ordering::SeqCst);
        self.total_compressed.fetch_add(compressed, std::sync::atomic::Ordering::SeqCst);
    }

    fn get_total_bytes(&self) -> u64 {
        self.total_bytes.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn get_total_compressed(&self) -> u64 {
        self.total_compressed.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn store_file_metadata(&self, file_id: String, metadata: FileMetadata) {
        if let Ok(mut files) = self.files.lock() {
            files.insert(file_id, metadata);
        }
    }

    fn list_all_files(&self) -> Vec<FileMetadata> {
        if let Ok(files) = self.files.lock() {
            files.values().cloned().collect()
        } else {
            vec![]
        }
    }

    fn get_file_metadata(&self, file_id: &str) -> Option<FileMetadata> {
        if let Ok(files) = self.files.lock() {
            files.get(file_id).cloned()
        } else {
            None
        }
    }
}

// ============ HANDLERS ============

async fn health_check() -> impl IntoResponse {
    (StatusCode::OK, "OK")
}

async fn list_files(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let files = state.list_all_files();
    let backend = if files.is_empty() {
        "local".to_string()
    } else {
        files.first().map(|f| f.storage_backend.clone()).unwrap_or_else(|| "local".to_string())
    };

    Json(ListFilesResponse {
        total: files.len(),
        files,
        storage_backend: backend,
    })
}

async fn get_file_info(
    Path(file_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    match state.get_file_metadata(&file_id) {
        Some(metadata) => (StatusCode::OK, Json(json!(metadata))).into_response(),
        None => (StatusCode::NOT_FOUND, Json(json!({"error": "File not found"}))).into_response(),
    }
}

async fn upload_file(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    while let Some(field) = multipart.next_field().await.ok().flatten() {
        // Get filename before consuming field
        let filename = field.name().unwrap_or("upload").to_string();
        
        if let Ok(bytes) = field.bytes().await {
            let data = bytes.to_vec();
            let file_id = Uuid::new_v4().to_string();
            let original_size = data.len() as u64;
            
            // Upload to storage backend
            match state.storage.upload_file(&file_id, &filename, &data).await {
                Ok(storage_meta) => {
                    let metadata = FileMetadata {
                        file_id: file_id.clone(),
                        filename: storage_meta.filename.clone(),
                        size_bytes: original_size,
                        compressed_bytes: storage_meta.compressed_size,
                        compression_ratio: storage_meta.compression_ratio,
                        uploaded_at: Utc::now().to_rfc3339(),
                        compression_method: storage_meta.compression_method.clone(),
                        storage_backend: storage_meta.storage_backend.clone(),
                    };

                    state.store_file_metadata(file_id.clone(), metadata.clone());
                    state.increment_files();
                    state.add_bytes(original_size, storage_meta.compressed_size);

                    return (
                        StatusCode::CREATED,
                        Json(UploadResponse {
                            file_id,
                            filename: storage_meta.filename,
                            size_bytes: original_size,
                            compressed_bytes: storage_meta.compressed_size,
                            compression_ratio: storage_meta.compression_ratio,
                            compression_method: storage_meta.compression_method,
                            storage_backend: storage_meta.storage_backend,
                        }),
                    )
                        .into_response();
                }
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": format!("Upload failed: {}", e)})),
                    )
                        .into_response();
                }
            }
        }
    }

    (StatusCode::BAD_REQUEST, Json(json!({"error": "No file provided"}))).into_response()
}

async fn get_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let files = state.list_all_files();
    let backend = if files.is_empty() {
        "local".to_string()
    } else {
        files.first().map(|f| f.storage_backend.clone()).unwrap_or_else(|| "local".to_string())
    };

    Json(StatusResponse {
        status: "healthy".to_string(),
        version: "1.0.0".to_string(),
        files_stored: state.get_files_count(),
        total_bytes: state.get_total_bytes(),
        total_compressed: state.get_total_compressed(),
        uptime_seconds: state.get_uptime(),
        storage_backend: backend,
    })
}

// ============ STORAGE FACTORY ============

async fn create_storage() -> Result<Arc<dyn StorageBackend>, Box<dyn std::error::Error>> {
    let config = StorageConfig::from_env()?;

    match config.backend {
        storage::StorageBackendType::Local => {
            let path = config.local_path.unwrap_or_else(|| "/tmp/kore-uploads".to_string());
            Ok(Arc::new(LocalStorageBackend::new(path)))
        }
        #[cfg(feature = "s3")]
        storage::StorageBackendType::S3 => {
            let bucket = config
                .s3_bucket
                .ok_or("S3_BUCKET environment variable not set")?;
            let region = config
                .s3_region
                .ok_or("AWS_REGION environment variable not set")?;
            Ok(Arc::new(S3StorageBackend::new(bucket, region)))
        }
        #[cfg(not(feature = "s3"))]
        storage::StorageBackendType::S3 => {
            Err("S3 backend requires 's3' feature to be enabled".into())
        }
    }
}

// ============ MAIN ============

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let storage = match create_storage().await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to initialize storage: {}", e);
            eprintln!("Falling back to local storage");
            Arc::new(LocalStorageBackend::new("/tmp/kore-uploads".to_string()))
        }
    };

    let backend_name = if cfg!(feature = "s3") {
        std::env::var("STORAGE_BACKEND").unwrap_or_else(|_| "local".to_string())
    } else {
        "local".to_string()
    };

    let app_state = AppState::new(storage).await;

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/files/list", get(list_files))
        .route("/api/v1/files/:file_id/info", get(get_file_info))
        .route("/api/v1/files/upload", post(upload_file))
        .route("/api/v1/status", get(get_status))
        .with_state(Arc::new(app_state));

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8000")
        .await
        .expect("Failed to bind to port 8000");

    println!("🚀 Kore Cloud API v1.0.0 running on http://0.0.0.0:8000");
    println!("   Storage Backend: {}", backend_name.to_uppercase());
    println!();
    println!("📋 Endpoints:");
    println!("   ✓ GET  /health");
    println!("   ✓ POST /api/v1/files/upload");
    println!("   ✓ GET  /api/v1/files/list");
    println!("   ✓ GET  /api/v1/files/{{file_id}}/info");
    println!("   ✓ GET  /api/v1/status");
    println!();
    println!("📊 Storage Configuration:");
    println!("   Backend: {}", backend_name);
    if backend_name == "s3" {
        if let Ok(bucket) = std::env::var("AWS_S3_BUCKET") {
            println!("   Bucket: {}", bucket);
        }
        if let Ok(region) = std::env::var("AWS_REGION") {
            println!("   Region: {}", region);
        }
    }
    println!();

    axum::serve(listener, app)
        .await
        .expect("Server failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_metadata_creation() {
        let meta = FileMetadata {
            file_id: "test-id".to_string(),
            filename: "test.kore".to_string(),
            size_bytes: 1000,
            compressed_bytes: 500,
            compression_ratio: 0.5,
            uploaded_at: "2024-01-01T00:00:00Z".to_string(),
            compression_method: "Hybrid".to_string(),
            storage_backend: "local".to_string(),
        };
        assert_eq!(meta.size_bytes, 1000);
        assert_eq!(meta.compression_ratio, 0.5);
    }

    #[tokio::test]
    async fn test_app_state_initialization() {
        let storage = Arc::new(LocalStorageBackend::new("/tmp".to_string()));
        let state = AppState::new(storage).await;
        assert_eq!(state.get_files_count(), 0);
        
        state.increment_files();
        assert_eq!(state.get_files_count(), 1);
        
        state.add_bytes(1000, 500);
        assert_eq!(state.get_total_bytes(), 1000);
        assert_eq!(state.get_total_compressed(), 500);
    }
}
